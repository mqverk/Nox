import "./globals.css";

export const metadata = {
  title: "NOX",
  description: "Bare-metal server management panel"
};

export default function RootLayout({
  children
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className="bg-midnight text-ink font-mono">{children}</body>
    </html>
  );
}
