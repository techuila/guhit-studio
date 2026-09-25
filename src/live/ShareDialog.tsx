// The live session dialog behind the Share button (DECISIONS D29).
//   Off: your name and Start live session, with how it works.
//   Hosting: the invite with Copy, the addresses, everyone with Remove, End session.
//   Joined or reconnecting: everyone, Save a copy, Leave.
import { useEffect, useRef, useState } from "react";
import type { Participant } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, Spinner, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { useListPresence } from "../ui/useListPresence";
import { Avatar } from "./Avatar";
import { NAME_MAX, cleanName, host as hostOf, others, possessive } from "./format";
import { useLive } from "./liveStore";
import { copyInvite, endSession, errorText, leaveSession, openJoin, saveCopy, startHosting } from "./session";
import s from "./live.module.css";

export function ShareDialog({ stage }: { stage?: PresenceStage }) {
  const live = useLive((st) => st.status.mode);
  // While it closes, the dialog keeps showing the state it closed in.
  const frozen = useRef(live);
  if (stage !== "exit") frozen.current = live;
  const mode = frozen.current;
  const form = useOffForm();
  const close = () => useLive.getState().closeDialog();
  const title = mode === "off" ? "Start a live session" : "Live session";
  const footer = mode === "off" ? <OffFooter form={form} /> : mode === "hosting" ? <HostingFooter /> : <GuestFooter />;
  return (
    <Dialog title={title} onClose={close} width={460} stage={stage} footer={footer}>
      {/* A new state cross-fades in (the key restarts the fade). */}
      <div key={mode === "reconnecting" ? "joined" : mode} className={s.stateBody}>
        {mode === "off" ? <OffBody form={form} /> : mode === "hosting" ? <HostingBody /> : <GuestBody />}
      </div>
    </Dialog>
  );
}

// ---------------------------------------------------------------- off

type OffForm = ReturnType<typeof useOffForm>;

/** Your name and Start: a name is needed before hosting, asked for right here. */
function useOffForm() {
  const profileName = useLive((st) => st.profile?.name ?? "");
  const [name, setNameState] = useState(profileName);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  // The profile can load after the dialog opens: fill it in until the user types.
  const touched = useRef(false);
  useEffect(() => {
    if (!touched.current) setNameState(profileName);
  }, [profileName]);
  const fail = (msg: string) => {
    setError(msg);
    inputRef.current?.focus();
  };
  const setName = (value: string) => {
    touched.current = true;
    setNameState(value);
    setError(null);
  };
  const start = async () => {
    if (busy) return;
    if (cleanName(name) === "") return fail("Add your name first. Others see it next to your pointer.");
    setBusy(true);
    let err: string | null = null;
    try {
      await startHosting(name);
    } catch (e) {
      err = errorText(e);
    }
    // Enabled again before the field takes the focus back.
    setBusy(false);
    if (err) fail(err);
  };
  return { name, setName, error, busy, start, inputRef };
}

function OffBody({ form }: { form: OffForm }) {
  return (
    <div className={s.stack}>
      <p className={s.lead}>
        People on the same network or VPN join with an invite you send them. The project and its history stay on this computer.
      </p>
      <label className={s.label} htmlFor="live-name">
        Your name
      </label>
      <input
        id="live-name"
        ref={form.inputRef}
        className={cx(s.input, form.error && s.inputBad)}
        value={form.name}
        maxLength={NAME_MAX}
        placeholder="How others see you"
        spellCheck={false}
        autoComplete="off"
        disabled={form.busy}
        data-autofocus={form.name.trim() === "" ? true : undefined}
        onChange={(e) => form.setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void form.start();
          }
        }}
      />
      {form.error ? (
        <p key={form.error} className={s.error} role="alert">
          {form.error}
        </p>
      ) : (
        <p className={s.hint}>Others see it next to your pointer and in the chat.</p>
      )}
    </div>
  );
}

function OffFooter({ form }: { form: OffForm }) {
  return (
    <>
      <Button variant="ghost" icon="join" className={s.footLeft} disabled={form.busy} onClick={openJoin}>
        Join one instead
      </Button>
      <Button onClick={() => useLive.getState().closeDialog()}>Cancel</Button>
      <Button variant="primary" icon={form.busy ? undefined : "people"} disabled={form.busy} onClick={() => void form.start()} data-autofocus>
        {form.busy ? (
          <span className={s.busyLabel}>
            <Spinner size={14} /> Starting
          </span>
        ) : (
          "Start live session"
        )}
      </Button>
    </>
  );
}

// ---------------------------------------------------------------- hosting

function HostingBody() {
  const invite = useLive((st) => st.status.invite);
  const addresses = useLive((st) => st.status.addresses);
  // Just started from this dialog: Copy is the next step, so it takes the focus.
  const rowRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const row = rowRef.current;
    if (row && !row.contains(document.activeElement)) row.querySelector<HTMLButtonElement>("button")?.focus({ preventScroll: true });
  }, []);
  return (
    <div className={s.stack}>
      <span className={s.label}>Invite</span>
      <div className={s.inviteRow} ref={rowRef}>
        <input className={cx(s.input, s.invite)} value={invite ?? ""} readOnly aria-label="Invite" onFocus={(e) => e.currentTarget.select()} spellCheck={false} />
        <CopyButton disabled={!invite} />
      </div>
      <p className={s.hint}>Anyone with the invite can join while the session runs. Send it to people on the same network or VPN.</p>
      {addresses.length > 0 ? (
        <p className={s.addresses}>
          <span>This computer</span>
          {addresses.map((a) => (
            <code key={a}>{a}</code>
          ))}
        </p>
      ) : null}
      <People removable />
    </div>
  );
}

/** Copy morphs into a drawn check for a moment. */
function CopyButton({ disabled }: { disabled: boolean }) {
  const [copied, setCopied] = useState(0);
  useEffect(() => {
    if (!copied) return;
    const t = window.setTimeout(() => setCopied(0), 1800);
    return () => window.clearTimeout(t);
  }, [copied]);
  return (
    <Button
      className={cx(s.copy, copied > 0 && s.copyDone)}
      disabled={disabled}
      data-autofocus
      onClick={() =>
        void copyInvite().then((ok) => {
          if (ok) setCopied((n) => n + 1);
        })
      }
    >
      <span className={s.copyInner}>
        {copied > 0 ? (
          <svg key={copied} width={16} height={16} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path className={s.drawnCheck} d="M4.5 10.5l3.6 3.6L15.5 6.5" pathLength={1} />
          </svg>
        ) : (
          <Icon name="copy" size={16} />
        )}
        <span>{copied > 0 ? "Copied" : "Copy"}</span>
      </span>
    </Button>
  );
}

// ---------------------------------------------------------------- guest

function GuestBody() {
  const mode = useLive((st) => st.status.mode);
  const hostName = useLive((st) => hostOf(st.status)?.name ?? null);
  const project = useLive((st) => st.status.project_name);
  const whose = hostName ? possessive(hostName) : "the host's";
  return (
    <div className={s.stack}>
      {mode === "reconnecting" ? (
        <p className={cx(s.lead, s.reconnecting)} role="status">
          <Spinner size={14} />
          <span>
            The connection to {whose} computer dropped. Trying again. Changes wait until it is back.
          </span>
        </p>
      ) : (
        <p className={s.lead}>
          You are in {whose} live session{project ? <> on <strong>{project}</strong></> : null}. The project stays on {whose} computer, and your changes go there.
        </p>
      )}
      <People removable={false} />
    </div>
  );
}

// ---------------------------------------------------------------- people

function People({ removable }: { removable: boolean }) {
  const participants = useLive((st) => st.status.participants);
  const selfId = useLive((st) => st.status.self_id);
  const mode = useLive((st) => st.status.mode);
  const rows = useListPresence(participants, (p) => p.id, "base");
  return (
    <section className={s.people} aria-label="People in the session">
      <h3 className={s.label}>
        People <span className={s.count}>{participants.length}</span>
      </h3>
      <ul className={cx(s.peopleList, mode === "reconnecting" && s.peopleStale)}>
        {rows.map((row) => (
          <PersonRow key={row.key} p={row.item} self={row.item.id === selfId} removable={removable && row.item.id !== selfId} entering={row.entering} leaving={row.leaving} />
        ))}
      </ul>
      {participants.length <= 1 && mode === "hosting" ? <p className={s.hint}>No one has joined yet. Copy the invite and send it.</p> : null}
    </section>
  );
}

function PersonRow({ p, self, removable, entering, leaving }: { p: Participant; self: boolean; removable: boolean; entering: boolean; leaving: boolean }) {
  const [busy, setBusy] = useState(false);
  const remove = async () => {
    setBusy(true);
    try {
      useLive.getState().setStatus(await ipc.liveRemove(p.id));
    } catch (e) {
      useApp.getState().reportError(e);
      setBusy(false);
    }
  };
  return (
    <li className={cx(s.person, entering && s.personEnter, leaving && s.personLeave)}>
      <Avatar p={p} size={26} />
      <span className={s.personName}>{p.name}</span>
      {p.role === "host" ? <span className={s.tag}>host</span> : null}
      {self ? <span className={s.tag}>you</span> : null}
      {removable ? (
        <Button size="sm" variant="ghost" className={s.remove} disabled={busy || leaving} onClick={() => void remove()} aria-label={`Remove ${p.name} from the session`}>
          Remove
        </Button>
      ) : null}
    </li>
  );
}

// ---------------------------------------------------------------- footer

function HostingFooter() {
  const askEnd = useLive((st) => st.askEnd);
  const count = useLive((st) => others(st.status).length);
  const [busy, setBusy] = useState(false);
  const end = async () => {
    setBusy(true);
    useLive.getState().closeDialog();
    await endSession();
    setBusy(false);
  };
  // "End the session?" takes the focus, so Enter answers it.
  useEffect(() => {
    if (askEnd) document.querySelector<HTMLButtonElement>("[data-end-confirm]")?.focus();
  }, [askEnd]);
  if (askEnd) {
    return (
      <>
        <span className={cx(s.footLeft, s.footAsk)} role="alert">
          {count > 0 ? `End the session for ${count === 1 ? "the other person" : `the other ${count}`}?` : "End the live session?"}
        </span>
        <Button onClick={() => useLive.getState().setAskEnd(false)}>Keep it</Button>
        <Button variant="danger" disabled={busy} onClick={() => void end()} data-autofocus data-end-confirm>
          End session
        </Button>
      </>
    );
  }
  return (
    <>
      <Button variant="danger" className={s.footLeft} onClick={() => (count > 0 ? useLive.getState().setAskEnd(true) : void end())}>
        End session
      </Button>
      <Button variant="primary" onClick={() => useLive.getState().closeDialog()}>
        Done
      </Button>
    </>
  );
}

function GuestFooter() {
  const [busy, setBusy] = useState<"copy" | "leave" | null>(null);
  const run = async (what: "copy" | "leave") => {
    setBusy(what);
    try {
      if (what === "copy") await saveCopy();
      else await leaveSession();
    } finally {
      setBusy(null);
    }
  };
  return (
    <>
      <Button variant="danger" className={s.footLeft} icon="back" disabled={busy !== null} onClick={() => void run("leave")}>
        Leave
      </Button>
      <Button icon="folder" disabled={busy !== null} onClick={() => void run("copy")}>
        {busy === "copy" ? "Saving" : "Save a copy"}
      </Button>
      <Button variant="primary" onClick={() => useLive.getState().closeDialog()} data-autofocus>
        Done
      </Button>
    </>
  );
}
