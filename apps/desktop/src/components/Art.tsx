import { useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Music2 } from "lucide-react";

/** Album art from the cached `art://` protocol, with a graceful fallback. */
export function Art({ path, className }: { path: string | null | undefined; className?: string }) {
  const [failed, setFailed] = useState(false);
  if (!path || failed) {
    return (
      <div className={`flex items-center justify-center bg-[var(--bg-hover)] text-[var(--text-dim)] ${className ?? ""}`}>
        <Music2 size={18} strokeWidth={1.5} />
      </div>
    );
  }
  return (
    <img
      src={convertFileSrc(path, "art")}
      onError={() => setFailed(true)}
      draggable={false}
      className={`object-cover bg-[var(--bg-hover)] ${className ?? ""}`}
    />
  );
}
