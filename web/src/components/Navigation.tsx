import { labels } from "../labels";
export type View = "pipeline" | "packets" | "operations";
interface Props {
  view: View;
  onSelect: (view: View) => void;
}
const views: View[] = ["pipeline", "packets", "operations"];
function Icon({ view }: { view: View }) {
  return (
    <svg
      className="size-4 shrink-0 fill-none stroke-current stroke-[1.5]"
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      {view === "pipeline" ? (
        <>
          <rect x="2" y="2" width="6" height="6" rx="1" />
          <rect x="16" y="16" width="6" height="6" rx="1" />
          <path d="M5 8v8a3 3 0 0 0 3 3h8M16 5h3v6M12 5h1" />
        </>
      ) : (
        <path
          d={
            view === "packets"
              ? "M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5M7 12h10"
              : "M2 12h5l3-9 4 18 3-9h5"
          }
        />
      )}
    </svg>
  );
}
export function Navigation({ view, onSelect }: Props) {
  return (
    <nav
      className="am-nav flex flex-col gap-[5px] max-compact:flex-row max-compact:justify-between max-compact:gap-0.5"
      aria-label={labels.navigation}
    >
      {views.map((entry) => (
        <button
          className="flex items-center gap-[9px] rounded-md bg-transparent p-2.5 text-left text-[12px] aria-pressed:bg-accent-soft aria-pressed:text-accent max-compact:gap-[5px] max-compact:px-[7px] max-compact:py-[9px] max-compact:text-[11px]"
          type="button"
          key={entry}
          data-view={entry}
          aria-pressed={view === entry}
          onClick={() => onSelect(entry)}
        >
          <Icon view={entry} />
          <span>{labels[entry]}</span>
        </button>
      ))}
    </nav>
  );
}
