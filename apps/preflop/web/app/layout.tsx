import type { Metadata, Viewport } from "next";
import { headers } from "next/headers";
import "./globals.css";

export async function generateMetadata(): Promise<Metadata> {
  const requestHeaders = await headers();
  const forwardedHost = requestHeaders.get("x-forwarded-host")?.split(",")[0]?.trim();
  const host = forwardedHost || requestHeaders.get("host") || "localhost:3000";
  const forwardedProtocol = requestHeaders.get("x-forwarded-proto")?.split(",")[0]?.trim();
  const protocol = forwardedProtocol === "http" || host.startsWith("localhost") || host.startsWith("127.0.0.1")
    ? "http"
    : "https";
  let metadataBase: URL;
  try {
    metadataBase = new URL(`${protocol}://${host}`);
  } catch {
    metadataBase = new URL("http://localhost:3000");
  }
  const imageUrl = new URL("/og-multiway.png", metadataBase);

  return {
    metadataBase,
    title: "Solvers Lab — HU / Multiway プリフロップソルバー",
    description:
      "HUから最大9-maxまで、No-Limit Hold’emのプリフロップ解析を設計・実行するワークスペース。",
    openGraph: {
      title: "Solvers Lab — HU / Multiway プリフロップソルバー",
      description: "最大9-maxのスポット、全streetベットツリー、ChipEV / ICMをひとつの画面で設計。",
      type: "website",
      url: metadataBase,
      images: [
        {
          url: imageUrl,
          width: 1730,
          height: 909,
          alt: "Solvers Lab 9-max Multiway Preflop Solver",
        },
      ],
    },
    twitter: {
      card: "summary_large_image",
      title: "Solvers Lab — HU / Multiway プリフロップソルバー",
      description: "最大9-maxのプリフロップ解析を設計・実行。",
      images: [imageUrl],
    },
  };
}

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#132621",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="ja">
      <body>{children}</body>
    </html>
  );
}
