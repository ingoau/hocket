// ARIA tabs: a tablist whose selected tab is the one Tab stop; Left/Right
// (and Home/End) move between tabs and select them (automatic activation);
// each tab controls a tabpanel labelled by it.
import { useRef } from "react";

export interface TabDef<K extends string> {
  key: K;
  label: string;
  testId?: string;
}

export function Tabs<K extends string>({ id, tabs, selected, onSelect, className = "tabs", tabClassName = "tab", label }: { id: string; tabs: TabDef<K>[]; selected: K; onSelect: (k: K) => void; className?: string; tabClassName?: string; label?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const onKey = (e: React.KeyboardEvent) => {
    const i = tabs.findIndex((x) => x.key === selected);
    let next: number | undefined;
    if (e.key === "ArrowRight") next = (i + 1) % tabs.length;
    else if (e.key === "ArrowLeft") next = (i - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    if (next === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    const tab = tabs[next] as TabDef<K>;
    onSelect(tab.key);
    ref.current?.querySelector<HTMLElement>(`#${id}-tab-${tab.key}`)?.focus();
  };
  return (
    <div ref={ref} className={className} role="tablist" aria-label={label} onKeyDown={onKey}>
      {tabs.map((tab) => (
        <button key={tab.key} id={`${id}-tab-${tab.key}`} type="button" role="tab" aria-selected={selected === tab.key} aria-controls={`${id}-panel`} tabIndex={selected === tab.key ? 0 : -1} className={`${tabClassName} ${selected === tab.key ? "active" : ""}`} onClick={() => onSelect(tab.key)} data-testid={tab.testId}>
          {tab.label}
        </button>
      ))}
    </div>
  );
}

/** Props for the panel a <Tabs id=…> controls. */
export function tabPanelProps(id: string, selected: string): { id: string; role: "tabpanel"; "aria-labelledby": string } {
  return { id: `${id}-panel`, role: "tabpanel", "aria-labelledby": `${id}-tab-${selected}` };
}
