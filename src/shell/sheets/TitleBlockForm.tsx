// The Title block tab of the Sheets panel (lane L4): the PH title block
// printed on every sheet (DECISIONS D53). Project fields go through
// `SetTitleBlock`, the signing architect through `SetSignatory`, which only
// people send: agents and the copilot are refused. The license numbers come
// from this computer's profile only when the user clicks "Fill from my
// profile"; "Save as my profile" keeps them for the next project. Each edit
// lands on the latest title block (commitLatest.ts), so fields typed quickly
// one after another never put an earlier one back.
import { useEffect, useState } from "react";
import type { DocState, Signatory, TitleBlock } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { useApp } from "../../state/store";
import { Button, TextField } from "../../ui/controls";
import { editSignatory, editTitleBlock } from "../commitLatest";
import { Icon } from "../../ui/icons";
import { pickImage } from "./pickImage";
import s from "./sheets.module.css";

const SIGNATORY_FIELDS: Array<{ key: keyof Signatory; label: string; placeholder?: string }> = [
  { key: "prc_no", label: "PRC reg. no." },
  { key: "prc_valid_until", label: "Valid until", placeholder: "YYYY-MM-DD" },
  { key: "ptr_no", label: "PTR no." },
  { key: "ptr_date", label: "PTR date", placeholder: "YYYY-MM-DD" },
  { key: "ptr_place", label: "PTR place", placeholder: "Quezon City" },
  { key: "iapoa_no", label: "IAPOA no." },
  { key: "iapoa_or_no", label: "IAPOA O.R. no." },
  { key: "iapoa_date", label: "O.R. date", placeholder: "YYYY-MM-DD" },
  { key: "tin", label: "TIN", placeholder: "000-000-000-000" },
];

function Cell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className={s.cell}>
      <span className={s.cellLabel}>{label}</span>
      {children}
    </label>
  );
}

export function TitleBlockForm({ doc }: { doc: DocState }) {
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);
  const tb = doc.project.title_block;
  const sig = tb.signatory;
  const settings = doc.project.settings;
  const [logoUrl, setLogoUrl] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    if (!tb.firm_logo) {
      setLogoUrl(null);
      return;
    }
    ipc
      .fileData(tb.firm_logo)
      .then((url) => live && setLogoUrl(url))
      .catch(() => live && setLogoUrl(null));
    return () => {
      live = false;
    };
  }, [tb.firm_logo]);

  const setBlock = (patch: Partial<TitleBlock>) => editTitleBlock(tb, { ...tb, ...patch });
  const setSignatory = (next: Signatory) => editSignatory(sig, next);

  const fill = async () => {
    try {
      const profile = await ipc.professionalGet();
      if (Object.values(profile).every((v) => v.trim() === "")) {
        toast("info", "No profile on this computer yet. Fill in the fields, then Save as my profile.");
        return;
      }
      // Nothing to change is filled too.
      const unchanged = JSON.stringify(profile) === JSON.stringify(sig);
      if ((await setSignatory(profile)) || unchanged) toast("success", "Filled from your profile on this computer.");
    } catch (e) {
      reportError(e);
    }
  };
  const keep = async () => {
    try {
      await ipc.professionalSet(sig);
      toast("success", "Saved as your profile on this computer. It never leaves it.");
    } catch (e) {
      reportError(e);
    }
  };

  return (
    <>
      <div className={s.group}>
        <div className={s.groupHead}>Project</div>
        <Cell label="Project title">
          <TextField label="Project title" value={tb.project_title} placeholder={doc.project.name} onCommit={(project_title) => setBlock({ project_title })} />
        </Cell>
        <div className={s.grid2}>
          <Cell label="Owner">
            <span className={s.readOnly}>{settings.client_name || "Set in Project"}</span>
          </Cell>
          <Cell label="Location">
            <span className={s.readOnly}>{settings.location || "Set in Project"}</span>
          </Cell>
        </div>
        <div className={s.grid3}>
          <Cell label="Lot no.">
            <TextField label="Lot number" value={tb.lot_no} onCommit={(lot_no) => setBlock({ lot_no })} />
          </Cell>
          <Cell label="Block no.">
            <TextField label="Block number" value={tb.block_no} onCommit={(block_no) => setBlock({ block_no })} />
          </Cell>
          <Cell label="TCT no.">
            <TextField label="TCT number" value={tb.tct_no} onCommit={(tct_no) => setBlock({ tct_no })} />
          </Cell>
        </div>
      </div>

      <div className={s.group}>
        <div className={s.groupHead}>Firm</div>
        <Cell label="Firm name">
          <TextField label="Firm name" value={tb.firm_name} onCommit={(firm_name) => setBlock({ firm_name })} />
        </Cell>
        <Cell label="Firm address">
          <TextField label="Firm address" value={tb.firm_address} onCommit={(firm_address) => setBlock({ firm_address })} />
        </Cell>
        <div className={s.logo}>
          {logoUrl ? <img className={s.logoImg} src={logoUrl} alt="Firm logo" /> : null}
          <Button
            size="sm"
            icon="import"
            onClick={async () => {
              try {
                const file = await pickImage();
                if (file) await setBlock({ firm_logo: file });
              } catch (e) {
                reportError(e);
              }
            }}
          >
            {tb.firm_logo ? "Replace logo" : "Add logo"}
          </Button>
          {tb.firm_logo ? (
            <Button size="sm" variant="ghost" onClick={() => setBlock({ firm_logo: null })}>
              Remove
            </Button>
          ) : null}
        </div>
      </div>

      <div className={s.group}>
        <div className={s.groupHead}>Signing architect</div>
        <div className={s.privacy}>
          <Icon name="lock" size={14} />
          <span>Only you set these. Agents and the copilot cannot, and they see the numbers masked. The PDF you export prints them in full; a field left empty prints blank.</span>
        </div>
        <Cell label="Name">
          <TextField label="Architect's name" value={sig.name} placeholder="Arch. Juan Dela Cruz" onCommit={(name) => setSignatory({ ...sig, name })} />
        </Cell>
        <div className={s.grid2}>
          {SIGNATORY_FIELDS.map((f) => (
            <Cell key={f.key} label={f.label}>
              <TextField label={f.label} value={sig[f.key]} placeholder={f.placeholder} onCommit={(v) => setSignatory({ ...sig, [f.key]: v })} />
            </Cell>
          ))}
        </div>
        <div className={s.buttons}>
          <Button size="sm" icon="people" onClick={() => void fill()}>
            Fill from my profile
          </Button>
          <Button size="sm" icon="check" onClick={() => void keep()}>
            Save as my profile
          </Button>
        </div>
      </div>
    </>
  );
}
