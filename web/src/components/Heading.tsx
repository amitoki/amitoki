import type { ReactNode } from "react";
export function Heading({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="am-heading mb-[18px] flex flex-wrap items-center justify-between gap-3 max-compact:items-start">
      <h2>{title}</h2>
      {children}
    </div>
  );
}
