// Command palette (⌘K / Ctrl+K).

import { clear, el } from "./dom";
import { t } from "./i18n";
import { closeOverlay, openOverlay } from "./modals";

export interface PaletteAction {
  id: string;
  label: string;
  hint?: string;
  run(): void | Promise<void>;
}

let paletteSeq = 0;

export function showPalette(actions: PaletteAction[]): void {
  const listId = `palette-list-${++paletteSeq}`;
  const input = el("input", {
    placeholder: t("palette.placeholder"),
    "aria-label": t("palette.label"),
    autocomplete: "off",
    role: "combobox",
    "aria-expanded": "true",
    "aria-controls": listId,
    "aria-autocomplete": "list",
  });
  const list = el("ul", { class: "palette-list", role: "listbox", id: listId });
  const host = el("div", { class: "palette", role: "dialog", "aria-label": t("palette.label") }, [
    input,
    list,
  ]);

  let filtered: PaletteAction[] = actions;
  let activeIndex = 0;

  const run = (action: PaletteAction) => {
    closeOverlay();
    void action.run();
  };

  const draw = () => {
    clear(list);
    if (filtered.length === 0) {
      input.removeAttribute("aria-activedescendant");
      list.append(
        el("li", { role: "presentation" }, [
          el("div", { class: "note palette-empty", text: t("palette.empty") }),
        ]),
      );
      return;
    }
    filtered.forEach((action, i) => {
      const item = el("div", {
        class: `palette-item${i === activeIndex ? " active" : ""}`,
        role: "option",
        id: `${listId}-option-${i}`,
        "aria-selected": String(i === activeIndex),
      });
      item.append(
        el("span", { text: action.label }),
        action.hint ? el("span", { class: "hint", text: action.hint }) : el("span"),
      );
      item.addEventListener("click", () => run(action));
      item.addEventListener("mousemove", () => {
        if (activeIndex !== i) {
          activeIndex = i;
          draw();
        }
      });
      list.append(el("li", { role: "presentation" }, [item]));
    });
    input.setAttribute("aria-activedescendant", `${listId}-option-${activeIndex}`);
    list.querySelector<HTMLElement>(".palette-item.active")?.scrollIntoView({ block: "nearest" });
  };

  const applyFilter = (query: string) => {
    const q = query.trim().toLowerCase();
    filtered = q
      ? actions.filter((a) =>
          `${a.label} ${a.id} ${a.hint ?? ""}`.toLowerCase().includes(q),
        )
      : actions;
    activeIndex = 0;
    draw();
  };

  input.addEventListener("input", () => applyFilter(input.value));
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") {
      activeIndex = Math.min(activeIndex + 1, filtered.length - 1);
      draw();
      e.preventDefault();
    } else if (e.key === "ArrowUp") {
      activeIndex = Math.max(activeIndex - 1, 0);
      draw();
      e.preventDefault();
    } else if (e.key === "Enter") {
      const action = filtered[activeIndex];
      if (action) run(action);
      e.preventDefault();
    }
  });

  applyFilter("");
  openOverlay(host);
  input.focus();
}
