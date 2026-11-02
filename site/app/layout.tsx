import type { Metadata } from "next";
import { Geist_Mono } from "next/font/google";
import "./globals.css";

const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"] });

const title = "vanityctl: agent-managed self-hosted hardware";
const description =
  "Give Codex, Claude Code or another coding agent one setup prompt. It inventories your host, plans the migration and operates services through a lasting registry and CLI.";

export const metadata: Metadata = {
  metadataBase: new URL("https://bryanhu.com/vanityctl/"),
  title,
  description,
  icons: {
    icon: "https://bryanhu.com/vanityctl/vanityctl-logo.png",
  },
  openGraph: {
    title,
    description,
    type: "website",
    url: "https://bryanhu.com/vanityctl/",
    images: [
      {
        url: "https://bryanhu.com/vanityctl/og.png",
        width: 1200,
        height: 630,
        alt: "vanityctl: Agent-managed self-hosted hardware. Copy the setup prompt for your coding agent.",
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    title,
    description,
    images: ["https://bryanhu.com/vanityctl/og.png"],
  },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body className={geistMono.variable}>{children}</body>
    </html>
  );
}
