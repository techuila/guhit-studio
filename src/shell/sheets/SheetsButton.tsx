// The top bar's Sheets button (lane L4): opens the Sheets panel on the whole
// set (`open_sheets`). It folds to its icon on a narrow top bar like the
// Versions button.
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { Button } from "../../ui/controls";
import chrome from "../chrome.module.css";

export function SheetsButton() {
  const count = useApp((st) => st.doc?.project.sheet_set.sheets.length ?? 0);
  return (
    <Button
      variant="chrome"
      icon="schedule"
      className={chrome.foldable}
      data-tip={count > 0 ? `Sheets: the permit set, ${count} ${count === 1 ? "sheet" : "sheets"}` : "Sheets: make the permit set"}
      onClick={() => bus.emit("open_sheets", { sheet_id: null })}
    >
      Sheets
    </Button>
  );
}
