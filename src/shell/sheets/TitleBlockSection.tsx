// The PH title block in the project inspector (lane L4): what every sheet
// prints (lot, block, TCT, the signing architect) at a glance, and the way
// into the Sheets panel where it is edited. The license numbers are set
// only by people, there (DECISIONS D53).
import type { DocState } from "../../contract/bindings";
import { bus } from "../../state/bus";
import { Button, Section } from "../../ui/controls";
import { useSection } from "../shellStore";
import s from "./sheets.module.css";

export function TitleBlockSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("title_block", false);
  const tb = doc.project.title_block;
  const sheets = doc.project.sheet_set.sheets.length;
  const row = (label: string, value: string) => (
    <div>
      <dt>{label}</dt>
      <dd title={value}>{value.trim() || "Not set"}</dd>
    </div>
  );
  return (
    <Section title="Title block and sheets" icon="schedule" count={sheets} open={open} onToggle={toggle}>
      <dl className={s.summary}>
        {row("Lot, block", [tb.lot_no, tb.block_no].filter((x) => x.trim()).join(", "))}
        {row("TCT", tb.tct_no)}
        {row("Architect", tb.signatory.name)}
        {row("Firm", tb.firm_name)}
      </dl>
      <Button size="sm" icon="schedule" onClick={() => bus.emit("open_sheets", { sheet_id: null })}>
        {sheets > 0 ? "Open sheets" : "Make the permit set"}
      </Button>
    </Section>
  );
}
