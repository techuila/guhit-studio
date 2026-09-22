// Project hub: the first screen. Lists local projects and creates new ones.
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { IpcError, ProjectMeta } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { useShell } from "../shell/shellStore";
import { useApp } from "../state/store";
import { ConfirmDialog, Dialog, Menu } from "../ui/Dialog";
import { Button, IconButton, Spinner, cx } from "../ui/controls";
import { setBusyLabel } from "../ui/feedback";
import { BrandMark, Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { Presence, useLastTruthy } from "../ui/motionDom";
import { formatArea, relativeTime } from "../ui/units";
import { BlankSketch, BrandDrawing, PlanSketch } from "./PlanSketch";
import s from "./ProjectHub.module.css";

type Template = "blank" | "sample-bungalow";

export function ProjectHub() {
  const openProject = useApp((st) => st.openProject);
  const createProject = useApp((st) => st.createProject);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);

  const [projects, setProjects] = useState<ProjectMeta[] | null>(null);
  const [loadError, setLoadError] = useState<IpcError | null>(null);
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState<Template | null>(null);
  const lastCreating = useLastTruthy(creating);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [menuId, setMenuId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<ProjectMeta | null>(null);
  const lastDeleting = useLastTruthy(deleting);

  const refresh = useCallback(async () => {
    try {
      setProjects(await ipc.hubList());
      setLoadError(null);
    } catch (e) {
      setLoadError(toIpcError(e));
      setProjects((p) => p ?? []);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = projects ?? [];
    return q ? list.filter((p) => p.name.toLowerCase().includes(q)) : list;
  }, [projects, query]);

  const rename = async (p: ProjectMeta, name: string) => {
    setRenamingId(null);
    const next = name.trim();
    if (next === "" || next === p.name) return;
    try {
      const meta = await ipc.hubRename(p.id, next);
      setProjects((list) => (list ?? []).map((x) => (x.id === p.id ? meta : x)));
    } catch (e) {
      reportError(e);
    }
  };

  const duplicate = async (p: ProjectMeta) => {
    try {
      const meta = await ipc.hubDuplicate(p.id);
      toast("success", `Duplicated as "${meta.name}"`);
      await refresh();
    } catch (e) {
      reportError(e);
    }
  };

  const remove = async (p: ProjectMeta) => {
    setDeleting(null);
    try {
      await ipc.hubDelete(p.id);
      setProjects((list) => (list ?? []).filter((x) => x.id !== p.id));
      toast("info", `"${p.name}" moved to trash`);
    } catch (e) {
      reportError(e);
    }
  };

  const loading = projects === null;
  const empty = !loading && !loadError && (projects?.length ?? 0) === 0;

  return (
    <div className={s.hub}>
      <aside className={s.brand}>
        <div className={s.brandTop}>
          <BrandMark size={30} />
          <div className={s.wordmark}>
            <span className={s.wordmarkName}>GUHIT</span>
            <span className={s.wordmarkSub}>Studio</span>
          </div>
        </div>
        <p className={s.tagline}>
          Draw. See.
          <br />
          Build the idea.
        </p>
        <BrandDrawing className={s.brandDrawing} />
        <p className={s.brandFoot}>Projects are saved on this computer.</p>
      </aside>

      <main className={s.main}>
        <header className={s.mainHead}>
          <h1>Projects</h1>
          {projects && projects.length > 0 ? <span className={s.count}>{projects.length}</span> : null}
          <div className={s.spacer} />
          {projects && projects.length > 3 ? (
            <label className={s.search}>
              <Icon name="search" size={15} />
              <input
                type="text"
                value={query}
                placeholder="Find a project"
                aria-label="Find a project"
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
          ) : null}
          <Button icon="import" onClick={() => useShell.getState().requestImport("bundle")}>
            Open bundle
          </Button>
          <Button variant="primary" icon="plus" onClick={() => setCreating("blank")}>
            New project
          </Button>
        </header>

        <div className={s.scroll}>
          {loading ? (
            <div className={s.state}>
              <Spinner size={20} />
            </div>
          ) : loadError ? (
            <div className={s.state}>
              <Icon name="warning" size={22} className={s.stateWarn} />
              <h2>Projects could not be loaded</h2>
              <p className={s.stateDetail}>{loadError.message}</p>
              <Button onClick={() => void refresh()}>Try again</Button>
            </div>
          ) : empty ? (
            <div className={s.state}>
              <div className={s.emptyArt}>
                <PlanSketch seed="welcome" />
              </div>
              <h2>Start your first plan</h2>
              <p>Draw the walls, see the house in 3D right away, then hand your client a clean drawing.</p>
              <div className={s.stateActions}>
                <Button variant="primary" icon="plus" onClick={() => setCreating("blank")}>
                  New project
                </Button>
                <Button onClick={() => setCreating("sample-bungalow")}>Start from the sample bungalow</Button>
              </div>
            </div>
          ) : shown.length === 0 ? (
            <div className={s.state}>
              <h2>No project matches "{query}"</h2>
              <Button onClick={() => setQuery("")}>Clear the search</Button>
            </div>
          ) : (
            <div className={s.grid}>
              <button type="button" className={s.newCard} onClick={() => setCreating("blank")}>
                <span className={s.newCardPlus}>
                  <Icon name="plus" size={22} />
                </span>
                <span>New project</span>
              </button>
              {shown.map((p, i) => (
                <ProjectCard
                  key={p.id}
                  project={p}
                  index={i}
                  renaming={renamingId === p.id}
                  menuOpen={menuId === p.id}
                  onOpen={() => {
                    setBusyLabel("Opening project");
                    void openProject(p.id);
                  }}
                  onMenu={(open) => setMenuId(open ? p.id : null)}
                  onStartRename={() => setRenamingId(p.id)}
                  onRename={(name) => void rename(p, name)}
                  onCancelRename={() => setRenamingId(null)}
                  onDuplicate={() => void duplicate(p)}
                  onDelete={() => setDeleting(p)}
                />
              ))}
            </div>
          )}
        </div>
      </main>

      <Presence open={creating !== null} exit="panel">
        {(stage) => {
          const template = lastCreating;
          return template ? (
            <CreateDialog
              initialTemplate={template}
              existingNames={(projects ?? []).map((p) => p.name)}
              onClose={() => setCreating(null)}
              onCreate={(name, t) => {
                setCreating(null);
                setBusyLabel("Creating project");
                void createProject(name, t);
              }}
              stage={stage}
            />
          ) : null;
        }}
      </Presence>

      <Presence open={deleting !== null} exit="panel">
        {(stage) => {
          const shown = lastDeleting;
          return shown ? (
            <ConfirmDialog
              title="Move project to trash?"
              message={
                <>
                  <strong>{shown.name}</strong> will be moved to the trash folder inside the app data. It is not erased,
                  so it can be recovered from there.
                </>
              }
              confirmLabel="Move to trash"
              danger
              onConfirm={() => void remove(shown)}
              onCancel={() => setDeleting(null)}
              stage={stage}
            />
          ) : null;
        }}
      </Presence>
    </div>
  );
}

interface CardProps {
  project: ProjectMeta;
  /** Position in the grid, for a first-appearance stagger (capped at 8 items). */
  index: number;
  renaming: boolean;
  menuOpen: boolean;
  onOpen: () => void;
  onMenu: (open: boolean) => void;
  onStartRename: () => void;
  onRename: (name: string) => void;
  onCancelRename: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}

function ProjectCard(p: CardProps) {
  const { project } = p;
  const closeMenu = useCallback(() => p.onMenu(false), [p]);
  return (
    <div
      className={cx(s.card, p.menuOpen && s.cardMenuOpen, p.index < 8 && s.cardRise)}
      style={p.index < 8 ? ({ "--i": p.index } as CSSProperties) : undefined}
      role="button"
      tabIndex={0}
      aria-label={`Open ${project.name}`}
      onClick={() => {
        if (!p.renaming) p.onOpen();
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          p.onOpen();
        } else if (e.key === "F2") {
          p.onStartRename();
        }
      }}
    >
      <div className={s.thumb}>
        {project.thumbnail ? (
          <img src={project.thumbnail} alt="" draggable={false} />
        ) : (
          <PlanSketch seed={project.id} className={s.thumbSketch} />
        )}
      </div>
      <div className={s.cardBody}>
        <div className={s.cardTitleRow}>
          {p.renaming ? (
            <RenameInput initial={project.name} onCommit={p.onRename} onCancel={p.onCancelRename} />
          ) : (
            <span
              className={s.cardName}
              title="Double-click to rename"
              onDoubleClick={(e) => {
                e.stopPropagation();
                p.onStartRename();
              }}
            >
              {project.name}
            </span>
          )}
          <span className={s.cardMenuAnchor} onClick={(e) => e.stopPropagation()}>
            <IconButton
              icon="more"
              label={`Actions for ${project.name}`}
              tip="Rename, duplicate, delete"
              tipSide="top-end"
              data-tip-off={p.menuOpen ? "true" : undefined}
              active={p.menuOpen}
              onClick={() => p.onMenu(!p.menuOpen)}
            />
            <Presence open={p.menuOpen} exit="hover">
              {(stage) => (
                <Menu
                  onClose={closeMenu}
                  stage={stage}
                  items={[
                    { key: "open", label: "Open", icon: "folder", onSelect: p.onOpen },
                    { key: "rename", label: "Rename", icon: "pencil", onSelect: p.onStartRename },
                    { key: "duplicate", label: "Duplicate", icon: "copy", onSelect: p.onDuplicate },
                    { key: "delete", label: "Move to trash", icon: "trash", danger: true, onSelect: p.onDelete },
                  ]}
                />
              )}
            </Presence>
          </span>
        </div>
        <div className={s.cardMeta}>
          <span className={s.metaStrong}>{formatArea(project.floor_area_m2)}</span>
          <span>
            {project.room_count} {project.room_count === 1 ? "room" : "rooms"}
          </span>
          <span className={s.metaTime}>{relativeTime(project.updated_at)}</span>
        </div>
      </div>
    </div>
  );
}

function RenameInput({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  return (
    <input
      className={s.renameInput}
      value={value}
      autoFocus
      aria-label="Project name"
      spellCheck={false}
      onFocus={(e) => e.currentTarget.select()}
      onClick={(e) => e.stopPropagation()}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => {
        if (!done.current) onCommit(value);
      }}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          done.current = true;
          onCommit(value);
        } else if (e.key === "Escape") {
          done.current = true;
          onCancel();
        }
      }}
    />
  );
}

function uniqueName(base: string, existing: string[]): string {
  if (!existing.includes(base)) return base;
  for (let i = 2; i < 500; i++) {
    const candidate = `${base} ${i}`;
    if (!existing.includes(candidate)) return candidate;
  }
  return base;
}

function CreateDialog({
  initialTemplate,
  existingNames,
  onClose,
  onCreate,
  stage,
}: {
  initialTemplate: Template;
  existingNames: string[];
  onClose: () => void;
  onCreate: (name: string, template: Template) => void;
  stage?: PresenceStage;
}) {
  const [template, setTemplate] = useState<Template>(initialTemplate);
  const defaultName = (t: Template) => uniqueName(t === "blank" ? "Untitled house" : "Sample bungalow", existingNames);
  const [name, setName] = useState(() => defaultName(initialTemplate));
  const [touched, setTouched] = useState(false);
  const valid = name.trim() !== "";

  const pick = (t: Template) => {
    setTemplate(t);
    if (!touched) setName(defaultName(t));
  };

  return (
    <Dialog
      title="New project"
      onClose={onClose}
      width={520}
      stage={stage}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!valid} onClick={() => onCreate(name.trim(), template)}>
            Create project
          </Button>
        </>
      }
    >
      <form
        className={s.createForm}
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) onCreate(name.trim(), template);
        }}
      >
        <label className={s.createLabel} htmlFor="new-project-name">
          Project name
        </label>
        <input
          id="new-project-name"
          className={s.createInput}
          value={name}
          data-autofocus
          spellCheck={false}
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => {
            setName(e.target.value);
            setTouched(true);
          }}
        />

        <span className={s.createLabel}>Start from</span>
        <div className={s.templates} role="radiogroup" aria-label="Template">
          <button
            type="button"
            role="radio"
            aria-checked={template === "blank"}
            className={cx(s.template, template === "blank" && s.templateOn)}
            onClick={() => pick("blank")}
          >
            <span className={s.templateArt}>
              <BlankSketch />
            </span>
            <span className={s.templateName}>Blank</span>
            <span className={s.templateHint}>An empty sheet at 1:100. Draw your own walls.</span>
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={template === "sample-bungalow"}
            className={cx(s.template, template === "sample-bungalow" && s.templateOn)}
            onClick={() => pick("sample-bungalow")}
          >
            <span className={s.templateArt}>
              <PlanSketch seed="b" />
            </span>
            <span className={s.templateName}>Sample bungalow</span>
            <span className={s.templateHint}>A small furnished house with a gable roof, to explore and edit.</span>
          </button>
        </div>
      </form>
    </Dialog>
  );
}
