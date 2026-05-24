use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    process::{ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use sysinfo::{Pid as SysPid, System};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::sleep;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
enum Status {
    Starting,
    Online,
    Stopping,
    Offline,
    Crashed,
}

#[derive(Debug)]
struct ManagedProcess {
    id: Uuid,
    name: String,
    command: String,
    args: Vec<String>,
    pid: Option<i32>,
    status: Status,
    cpu: f32,
    memory: u64,
    start_time: Option<Instant>,
    restarts: u32,
    auto_restart: bool,
    tx: broadcast::Sender<String>,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
}

impl ManagedProcess {
    fn info(&self) -> ProcessInfo {
        let uptime = self
            .start_time
            .map(|instant| instant.elapsed().as_secs())
            .unwrap_or(0);
        ProcessInfo {
            id: self.id,
            name: self.name.clone(),
            status: self.status,
            pid: self.pid,
            cpu: self.cpu,
            memory: self.memory,
            uptime,
            restarts: self.restarts,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProcessInfo {
    id: Uuid,
    name: String,
    status: Status,
    pid: Option<i32>,
    cpu: f32,
    memory: u64,
    uptime: u64,
    restarts: u32,
}

#[derive(Debug, Deserialize)]
struct StartRequest {
    name: String,
    command: String,
    args: Option<Vec<String>>,
    auto_restart: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

enum ProcessEvent {
    Exited { id: Uuid, code: Option<i32> },
}

#[derive(Clone)]
struct AppState {
    processes: Arc<RwLock<HashMap<Uuid, ManagedProcess>>>,
    event_tx: mpsc::UnboundedSender<ProcessEvent>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let state = Arc::new(AppState {
        processes: Arc::new(RwLock::new(HashMap::new())),
        event_tx,
    });

    let event_state = state.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            handle_event(event, event_state.clone()).await;
        }
    });

    let telemetry_state = state.clone();
    tokio::spawn(async move {
        let mut system = System::new();
        loop {
            system.refresh_processes();
            system.refresh_cpu();
            let mut processes = telemetry_state.processes.write().await;
            for process in processes.values_mut() {
                if let Some(pid) = process.pid {
                    let sys_pid = SysPid::from_u32(pid as u32);
                    if let Some(sys_proc) = system.process(sys_pid) {
                        process.cpu = sys_proc.cpu_usage();
                        process.memory = sys_proc.memory() * 1024;
                    } else {
                        process.cpu = 0.0;
                        process.memory = 0;
                    }
                } else {
                    process.cpu = 0.0;
                    process.memory = 0;
                }
            }
            drop(processes);
            sleep(Duration::from_millis(900)).await;
        }
    });

    let app = Router::new()
        .route("/api/processes", get(list_processes).post(start_process))
        .route("/api/processes/:id/stop", post(stop_process))
        .route("/ws/:id", get(ws_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(Any),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("noxd listening on 0.0.0.0:8080");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn list_processes(State(state): State<Arc<AppState>>) -> Json<Vec<ProcessInfo>> {
    let processes = state.processes.read().await;
    let mut list: Vec<_> = processes.values().map(ManagedProcess::info).collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Json(list)
}

async fn start_process(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StartRequest>,
) -> Result<Json<ProcessInfo>, (StatusCode, Json<ErrorResponse>)> {
    let (tx, _) = broadcast::channel(1024);
    let id = Uuid::new_v4();
    let mut process = ManagedProcess {
        id,
        name: payload.name,
        command: payload.command,
        args: payload.args.unwrap_or_default(),
        pid: None,
        status: Status::Starting,
        cpu: 0.0,
        memory: 0,
        start_time: None,
        restarts: 0,
        auto_restart: payload.auto_restart.unwrap_or(true),
        tx,
        stdin: None,
    };

    let mut processes = state.processes.write().await;
    if let Err(err) = spawn_process(&mut process, &state.event_tx) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        ));
    }
    let info = process.info();
    processes.insert(process.id, process);
    Ok(Json(info))
}

async fn stop_process(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProcessInfo>, (StatusCode, Json<ErrorResponse>)> {
    let mut processes = state.processes.write().await;
    let process = processes.get_mut(&id).ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "process not found".to_string(),
        }),
    ))?;

    process.auto_restart = false;
    if let Some(pid) = process.pid {
        process.status = Status::Stopping;
        if let Err(err) = kill(Pid::from_raw(pid), Signal::SIGTERM) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: err.to_string(),
                }),
            ));
        }
    } else {
        process.status = Status::Offline;
    }

    Ok(Json(process.info()))
}

async fn ws_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let (tx, stdin) = {
        let processes = state.processes.read().await;
        match processes.get(&id) {
            Some(process) => (process.tx.clone(), process.stdin.clone()),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: "process not found".to_string(),
                    }),
                )
                    .into_response()
            }
        }
    };

    ws.on_upgrade(move |socket| handle_socket(socket, tx, stdin))
        .into_response()
}

async fn handle_socket(
    socket: WebSocket,
    tx: broadcast::Sender<String>,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = tx.subscribe();

    let mut send_task = tokio::spawn(async move {
        while let Ok(line) = rx.recv().await {
            if sender.send(Message::Text(line)).await.is_err() {
                break;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Some(stdin) = stdin.as_ref() {
                        forward_stdin(stdin.clone(), text.into_bytes());
                    } else {
                        warn!("stdin unavailable for websocket input");
                    }
                }
                Message::Binary(data) => {
                    if let Some(stdin) = stdin.as_ref() {
                        forward_stdin(stdin.clone(), data);
                    } else {
                        warn!("stdin unavailable for websocket input");
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }
}

fn forward_stdin(stdin: Arc<Mutex<ChildStdin>>, data: Vec<u8>) {
    tokio::task::spawn_blocking(move || {
        let mut locked = match stdin.lock() {
            Ok(lock) => lock,
            Err(err) => {
                error!("stdin lock poisoned: {err}");
                return;
            }
        };
        if let Err(err) = locked.write_all(&data) {
            error!("stdin write failed: {err}");
            return;
        }
        if let Err(err) = locked.flush() {
            error!("stdin flush failed: {err}");
        }
    });
}

fn spawn_process(
    process: &mut ManagedProcess,
    event_tx: &mpsc::UnboundedSender<ProcessEvent>,
) -> anyhow::Result<()> {
    let mut command = Command::new(&process.command);
    command.args(&process.args);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn()?;
    let pid = child.id() as i32;

    process.pid = Some(pid);
    process.status = Status::Online;
    process.start_time = Some(Instant::now());

    let stdin = child.stdin.take().map(|handle| Arc::new(Mutex::new(handle)));
    process.stdin = stdin;

    let tx = process.tx.clone();
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, tx.clone(), "");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, tx.clone(), "[stderr] ");
    }

    let id = process.id;
    let event_tx = event_tx.clone();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        let _ = event_tx.send(ProcessEvent::Exited { id, code });
    });

    Ok(())
}

fn spawn_reader<R: Read + Send + 'static>(
    reader: R,
    tx: broadcast::Sender<String>,
    prefix: &'static str,
) {
    std::thread::spawn(move || {
        let buffer = BufReader::new(reader);
        for line in buffer.lines() {
            match line {
                Ok(content) => {
                    let _ = tx.send(format!("{prefix}{content}\n"));
                }
                Err(err) => {
                    let _ = tx.send(format!("[read-error] {err}\n"));
                    break;
                }
            }
        }
    });
}

async fn handle_event(event: ProcessEvent, state: Arc<AppState>) {
    match event {
        ProcessEvent::Exited { id, code } => {
            let mut should_restart = false;
            {
                let mut processes = state.processes.write().await;
                if let Some(process) = processes.get_mut(&id) {
                    process.pid = None;
                    process.stdin = None;
                    process.cpu = 0.0;
                    process.memory = 0;

                    let exit_code = code.unwrap_or(0);
                    let was_stopping = process.status == Status::Stopping;
                    if was_stopping {
                        process.status = Status::Offline;
                    } else if exit_code == 0 {
                        process.status = Status::Offline;
                        should_restart = process.auto_restart;
                    } else {
                        process.status = Status::Crashed;
                        should_restart = process.auto_restart;
                    }

                    if should_restart {
                        process.status = Status::Starting;
                        process.restarts = process.restarts.saturating_add(1);
                    }
                } else {
                    warn!("received exit event for unknown process {id}");
                }
            }

            if should_restart {
                warn!("process {id} exited with {:?}, restarting", code);
                sleep(Duration::from_secs(1)).await;
                let mut processes = state.processes.write().await;
                if let Some(process) = processes.get_mut(&id) {
                    if process.auto_restart {
                        if let Err(err) = spawn_process(process, &state.event_tx) {
                            process.status = Status::Crashed;
                            error!("failed to restart process {id}: {err}");
                        }
                    }
                }
            } else {
                info!("process {id} exited with {:?}", code);
            }
        }
    }
}
