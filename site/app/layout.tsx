import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({ variable: "--font-geist-sans", subsets: ["latin"] });
const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"] });

export const metadata: Metadata = {
  metadataBase: new URL("https://bryanhu.com/vanityctl/"),
  title: "vanityctl — one control plane for this computer",
  description:
    "A single-node declarative control plane for Docker, native processes, scheduled jobs, Git deployments, and DNS.",
  icons: {
    icon: "https://bryanhu.com/vanityctl/vanityctl-logo.png",
  },
  openGraph: {
    title: "vanityctl — one control plane for this computer",
    description: "Everything this machine is responsible for, in one declarative registry.",
    type: "website",
    url: "https://bryanhu.com/vanityctl/",
    images: [
      {
        url: "https://bryanhu.com/vanityctl/og.png",
        width: 1200,
        height: 630,
        alt: "vanityctl — one control plane for this computer",
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    images: ["https://bryanhu.com/vanityctl/og.png"],
  },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body className={`${geistSans.variable} ${geistMono.variable}`}>{children}</body>
    </html>
  );
}
