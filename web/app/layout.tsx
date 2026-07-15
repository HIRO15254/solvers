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
  const imageUrl = new URL("/og.png", metadataBase);

  return {
    metadataBase,
    title: "Solvers Lab — プリフロップ設定",
    description:
      "Heads-up No-Limit Hold’em のプリフロップ解析設定を、迷わず組み立てられるワークスペース。",
    openGraph: {
      title: "Solvers Lab — プリフロップ設定",
      description: "スポット、ベットツリー、精度をひとつの画面で設計。",
      type: "website",
      url: metadataBase,
      images: [
        {
          url: imageUrl,
          width: 1733,
          height: 908,
          alt: "Solvers Lab Preflop Workbench",
        },
      ],
    },
    twitter: {
      card: "summary_large_image",
      title: "Solvers Lab — プリフロップ設定",
      description: "スポット、ベットツリー、精度をひとつの画面で設計。",
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
