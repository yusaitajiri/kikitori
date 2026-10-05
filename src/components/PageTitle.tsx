import type { ReactNode } from "react";

/** The big title at the top of a page, on white, with an optional line beside it and an action at the end. */
export function PageTitle({ title, sub, action, children }: { title: ReactNode; sub?: ReactNode; action?: ReactNode; children?: ReactNode }) {
  return (
    <div className="shrink-0 px-[18px] pt-1.5 pb-3">
      <div data-tauri-drag-region className="flex items-baseline gap-2.5">
        <h2 data-tauri-drag-region className="text-[24px] leading-tight font-bold tracking-tight">
          {title}
        </h2>
        {sub && (
          <span data-tauri-drag-region className="text-[12px] text-muted">
            {sub}
          </span>
        )}
        {action && <span className="ml-auto self-center">{action}</span>}
      </div>
      {children}
    </div>
  );
}
