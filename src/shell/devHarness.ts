// Dev-only harness. Never loaded in production builds (main.tsx guards it
// with import.meta.env.DEV).
//
//   ?mock=1      answer IPC calls in the browser from the golden fixture, so the
//                shell can be worked on without the Rust bridge. Only a few
//                commands are imitated, shallowly, with no derived recompute.
//                Everything else fails with an IpcError, like an early engine.
//   ?mock=empty  same, with no projects in the hub.
//   ?fixture=1   jump straight into the editor with the fixture loaded.
//   ?crash=NAME  throws inside one panel (plan, view3d, copilot, visuals,
//                inspector) to exercise its error boundary. Read by EditorShell.
//   ?live=MODE   with ?mock: a fake live session (demo, guest, reconnecting)
//                where two people move, select and chat. src/live/dev/liveMock.ts.
import type { Command, DocState, IpcError, ProjectMeta, SnapshotMeta } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { useViewer } from "../viewer3d/viewerStore";
import { useShell } from "./shellStore";

async function loadFixture(): Promise<DocState> {
  const mod = await import("../../fixtures/sample-bungalow.docstate.json");
  return structuredClone(mod.default) as unknown as DocState;
}

function fail(code: string, message: string): never {
  throw { code, message, element_ids: [] } satisfies IpcError;
}

function applyShallow(doc: DocState, command: Command): string {
  const p = doc.project;
  switch (command.type) {
    case "update_element": {
      const i = p.elements.findIndex((e) => e.id === command.element.id);
      if (i < 0) fail("not_found", "Element not found");
      p.elements[i] = command.element;
      return "Edit element";
    }
    case "delete_elements":
      p.elements = p.elements.filter((e) => !command.ids.includes(e.id));
      return "Delete";
    case "set_material":
      p.elements = p.elements.map((e) =>
        command.ids.includes(e.id) && "material_id" in e ? { ...e, material_id: command.material_id } : e,
      );
      return "Set material";
    case "set_roof":
      p.roof = command.roof;
      return "Change roof";
    case "set_project_settings":
      p.settings = command.settings;
      return "Project settings";
    case "update_level":
      p.levels = p.levels.map((l) => (l.id === command.level.id ? command.level : l));
      return "Edit level";
    case "set_layer":
      p.layers = p.layers.map((l) => (l.key === command.layer.key ? command.layer : l));
      return "Layer";
    case "batch":
      command.commands.forEach((c) => applyShallow(doc, c));
      return command.label;
    default:
      return fail("invalid", `"${command.type}" is not implemented in the mock engine`);
  }
}

async function installMock(empty: boolean) {
  const fixture = await loadFixture();
  const now = Date.now();
  const iso = (msAgo: number) => new Date(now - msAgo).toISOString();
  let metas: ProjectMeta[] = empty
    ? []
    : [
        { id: fixture.project.id, name: "Sample Bungalow", created_at: iso(9e8), updated_at: iso(4 * 60e3), floor_area_m2: 45.34, room_count: 2, thumbnail: null },
        { id: "p-2", name: "Reyes Residence, Antipolo", created_at: iso(9e8), updated_at: iso(3 * 3600e3), floor_area_m2: 128.6, room_count: 9, thumbnail: null },
        { id: "p-3", name: "Dela Cruz duplex", created_at: iso(9e8), updated_at: iso(26 * 3600e3), floor_area_m2: 96.0, room_count: 7, thumbnail: null },
        { id: "p-4", name: "Tagaytay rest house", created_at: iso(9e8), updated_at: iso(6 * 86400e3), floor_area_m2: 74.25, room_count: 5, thumbnail: null },
        { id: "p-5", name: "Studio unit fit-out", created_at: iso(9e8), updated_at: iso(40 * 86400e3), floor_area_m2: 28.5, room_count: 2, thumbnail: null },
      ];
  let doc: DocState | null = null;
  const undoStack: string[] = [];
  const redoStack: string[] = [];
  let snapshots: SnapshotMeta[] = [
    { id: "s-2", label: "Before moving the kitchen", created_at: iso(50 * 60e3), revision: 12, auto: false },
    { id: "s-1", label: "Auto save before restore", created_at: iso(5 * 3600e3), revision: 7, auto: true },
  ];

  const stamp = (d: DocState, label?: string): DocState => {
    d.revision += 1;
    d.can_undo = undoStack.length > 0;
    d.can_redo = redoStack.length > 0;
    d.undo_label = d.can_undo ? (label ?? d.undo_label) : null;
    d.redo_label = d.can_redo ? d.redo_label : null;
    return structuredClone(d);
  };

  const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
    hub_list: () => metas,
    hub_create: (a) => {
      doc = structuredClone(fixture);
      doc.project.name = String(a.name);
      // The mock has no plumbing: "plumbing-demo" opens as the plain sample.
      if (a.template !== "sample-bungalow" && a.template !== "plumbing-demo") {
        doc.project.elements = [];
        doc.derived = { walls: [], rooms: [], footprints: [], totals: { floor_area_m2: 0, gross_area_m2: 0, wall_length_m: 0, room_count: 0, door_count: 0, window_count: 0 }, issues: [], pipes: { fittings: [], penetrations: [], takeoff: [], total_length_m: 0, elbow_count: 0, tee_count: 0, sleeve_count: 0 }, schedule: [], review_resolved: [] };
      }
      return structuredClone(doc);
    },
    hub_open: (a) => {
      doc = structuredClone(fixture);
      doc.project.name = metas.find((m) => m.id === a.id)?.name ?? doc.project.name;
      doc.derived.issues = [
        { id: "i-1", severity: "warning", code: "room_no_window", message: "Bedroom has no window. Consider adding one for light and air.", element_ids: doc.project.elements.filter((e) => e.kind === "room").slice(0, 1).map((e) => e.id), location: null, status: "open", note: "" },
        { id: "i-2", severity: "info", code: "door_narrow", message: "A door is narrower than 800 mm. Check that furniture can pass.", element_ids: doc.project.elements.filter((e) => e.kind === "opening").slice(0, 1).map((e) => e.id), location: null, status: "open", note: "" },
      ];
      return structuredClone(doc);
    },
    hub_rename: (a) => {
      metas = metas.map((m) => (m.id === a.id ? { ...m, name: String(a.name) } : m));
      if (doc && doc.project.id === a.id) doc.project.name = String(a.name);
      return metas.find((m) => m.id === a.id) ?? fail("not_found", "Project not found");
    },
    hub_duplicate: (a) => {
      const src = metas.find((m) => m.id === a.id) ?? fail("not_found", "Project not found");
      const copy = { ...src, id: `p-${Math.random().toString(36).slice(2, 8)}`, name: `${src.name} copy`, updated_at: new Date().toISOString() };
      metas = [copy, ...metas];
      return copy;
    },
    hub_delete: (a) => {
      metas = metas.filter((m) => m.id !== a.id);
      return null;
    },
    hub_set_thumbnail: (a) => {
      metas = metas.map((m) => (m.id === a.id ? { ...m, thumbnail: String(a.png) } : m));
      return null;
    },
    hub_close: () => {
      doc = null;
      return null;
    },
    doc_state: () => (doc ? structuredClone(doc) : null),
    doc_apply: (a) => {
      if (!doc) fail("no_project", "No project is open");
      const before = JSON.stringify(doc);
      const label = applyShallow(doc, a.command as Command);
      undoStack.push(before);
      redoStack.length = 0;
      return { state: stamp(doc, label), diff: { added: [], modified: [], removed: [], summary: label } };
    },
    doc_undo: () => {
      const prev = undoStack.pop();
      if (!doc || !prev) fail("invalid", "Nothing to undo");
      redoStack.push(JSON.stringify(doc));
      const revision = doc.revision;
      doc = JSON.parse(prev) as DocState;
      doc.revision = revision;
      doc.redo_label = "last change";
      return stamp(doc);
    },
    doc_redo: () => {
      const next = redoStack.pop();
      if (!doc || !next) fail("invalid", "Nothing to redo");
      undoStack.push(JSON.stringify(doc));
      const revision = doc.revision;
      doc = JSON.parse(next) as DocState;
      doc.revision = revision;
      return stamp(doc, "last change");
    },
    snapshot_list: () => snapshots,
    snapshot_create: (a) => {
      const meta: SnapshotMeta = { id: `s-${snapshots.length + 1}`, label: String(a.label), created_at: new Date().toISOString(), revision: doc?.revision ?? 0, auto: false };
      snapshots = [meta, ...snapshots];
      return meta;
    },
    snapshot_restore: () => {
      doc = structuredClone(fixture);
      return structuredClone(doc);
    },
    catalog_assets: () => [
      { key: "bed-double", name: "Double bed", category: "furniture", width_mm: 1400, depth_mm: 1900, height_mm: 500, elevation_mm: 0 },
      { key: "bed-single", name: "Single bed", category: "furniture", width_mm: 900, depth_mm: 1900, height_mm: 500, elevation_mm: 0 },
      { key: "sofa-3", name: "Sofa, 3 seater", category: "furniture", width_mm: 2100, depth_mm: 900, height_mm: 800, elevation_mm: 0 },
      { key: "dining-6", name: "Dining table for 6", category: "furniture", width_mm: 1800, depth_mm: 900, height_mm: 750, elevation_mm: 0 },
      { key: "wc", name: "Water closet", category: "sanitary", width_mm: 400, depth_mm: 700, height_mm: 760, elevation_mm: 0 },
      { key: "lavatory", name: "Lavatory", category: "sanitary", width_mm: 500, depth_mm: 420, height_mm: 850, elevation_mm: 0 },
      { key: "kitchen-sink", name: "Kitchen sink", category: "kitchen", width_mm: 900, depth_mm: 600, height_mm: 900, elevation_mm: 0 },
      { key: "ref", name: "Refrigerator", category: "appliance", width_mm: 700, depth_mm: 700, height_mm: 1700, elevation_mm: 0 },
      { key: "car-sedan", name: "Sedan", category: "vehicle", width_mm: 1800, depth_mm: 4600, height_mm: 1450, elevation_mm: 0 },
      { key: "plant-pot", name: "Potted plant", category: "plant", width_mm: 500, depth_mm: 500, height_mm: 1200, elevation_mm: 0 },
    ],
    export_plan: (a) => ({ path: (a.path as string | null) ?? `/mock/exports/plan.${String(a.format)}`, scale_denominator: 100 }),
    export_image: (a) => ({ path: (a.path as string | null) ?? `/mock/exports/${String(a.name)}.png`, scale_denominator: null }),
  };

  const realFetch = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = /\/ipc\/([a-z_]+)$/.exec(url);
    if (!m) return realFetch(input, init);
    const args = init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : {};
    const json = (status: number, body: unknown) =>
      new Response(JSON.stringify(body ?? null), { status, headers: { "content-type": "application/json" } });
    const handler = handlers[m[1]];
    if (!handler) return json(400, { code: "invalid", message: `${m[1]} is not available in the mock`, element_ids: [] });
    try {
      return json(200, await handler(args));
    } catch (e) {
      return json(400, e);
    }
  };
  // No bridge, so no event stream: app events arrive only from the live
  // session mock below, pushed through `onmessage`.
  const streams = new Set<SilentEventSource>();
  class SilentEventSource {
    onmessage: ((m: MessageEvent) => void) | null = null;
    onerror: ((e: Event) => void) | null = null;
    constructor() {
      streams.add(this);
    }
    close() {
      streams.delete(this);
    }
  }
  (window as unknown as { EventSource: unknown }).EventSource = SilentEventSource;
  const { installLiveMock } = await import("../live/dev/liveMock");
  installLiveMock(
    {
      handlers,
      emit: (event) => streams.forEach((s) => s.onmessage?.({ data: JSON.stringify(event) } as MessageEvent)),
      openShared: () => handlers.hub_open({ id: fixture.project.id }) as DocState,
      closeShared: () => void handlers.hub_close({}),
      levelId: fixture.project.levels[0]?.id ?? "",
      projectId: fixture.project.id,
      projectName: "Sample Bungalow",
      wallIds: fixture.project.elements.filter((e) => e.kind === "wall").map((e) => e.id),
      roomIds: fixture.project.elements.filter((e) => e.kind === "room").map((e) => e.id),
    },
    new URLSearchParams(window.location.search).get("live"),
  );
  console.info("[dev] mock IPC installed");
}

export async function installDevHarness() {
  const params = new URLSearchParams(window.location.search);
  const mock = params.get("mock");
  if (mock) await installMock(mock === "empty");
  if (params.get("fixture")) {
    const doc = await loadFixture();
    useApp.getState().setDoc(doc);
    useApp.setState({ screen: "editor" });
  }
  // Handy in the console and in ui-check step files.
  (window as unknown as { __app: typeof useApp }).__app = useApp;
  (window as unknown as { __ipc: typeof ipc }).__ipc = ipc;
  (window as unknown as { __shell: typeof useShell }).__shell = useShell;
  (window as unknown as { __bus: typeof bus }).__bus = bus;
  (window as unknown as { __viewer: typeof useViewer }).__viewer = useViewer;
}
