---
name: guhit-swarm
description: "Draw a whole building in Guhit Studio with several sub-agents at once, through the Guhit MCP server's swarm tool. Use when the user asks Guhit to draft a whole house or building (two or more levels, or about 30 elements or more) and the guhit-studio MCP server is connected. The lead splits the brief into parts, each sub-agent draws its part into a draft with its own handle, and a person accepts the draft in the Guhit Studio window."
---

# Guhit swarm

A swarm draws one building with several agents at once (Guhit DECISIONS D56,
D57). You lead: you split the brief, start the swarm, hand each sub-agent its
part, watch, and tell the user when the draft is ready. Everything the swarm
draws goes into a draft. Nothing reaches the model until a person presses
Accept in the Guhit Studio window, and no agent can accept.

Every length is millimeters. The plan is +x east, +y north.

## When to swarm

- Swarm: a whole house or building, two or more levels, or about 30 elements
  or more (walls, rooms, openings, stairs, furniture).
- Work alone: one room, a fix, a change to a few elements, anything on a
  project with a live session (swarms do not run in one).
- One swarm at a time. If `swarm` action "status" shows one open, the user
  accepts or discards it first.

## How to split the brief

At most 6 parts. Two parts never draw the same kind of element on the same
level: give each its own levels or its own kinds (an empty list means all of
them). The app refuses a plan that breaks this, and says what to change.

- **Several storeys: split by level.** One part per level, every kind it
  needs. Add the storeys in `setup`, never in a part.
- **One storey: split by trade** on that level:
  1. shell: `wall`, `column`;
  2. rooms and openings: `room`, `opening`, after shell's "walls";
  3. roof and stairs: `roof`, `stair`, `slab`, after shell is "done";
  4. furnishing and notes: `asset`, `annotation`, `dimension`, after rooms
     is "done".
- **Order with milestones.** A part declares the milestones it reports
  (`milestones: ["walls"]`); another comes `after` it
  (`after: [{"part": "shell", "milestone": "walls"}]`). "done" is every
  part's last milestone. At least one part must be able to start at once.
- Furnishing usually spans every level with kind `asset` only.

## Steps for the lead

1. `project` action "open" (or "create" with template "blank"). Read the
   brief and `get_guide` topic "swarm".
2. `swarm` action "start" with `brief`, `summary`, `setup` and `parts`:

   ```json
   {"action": "start",
    "brief": "Two storey house, 3 bedrooms, 10 x 8 m",
    "summary": "Two storey house",
    "setup": {"levels": [{"name": "Second Floor", "height_mm": 3000}]},
    "parts": [
      {"key": "ground", "name": "Ground floor", "role": "Living, kitchen, stair",
       "scope": {"levels": ["Ground Floor"], "kinds": ["wall", "room", "opening", "column", "stair"]},
       "milestones": ["walls"], "instructions": "..."},
      {"key": "upper", "name": "Upper floor", "role": "Three bedrooms",
       "scope": {"levels": ["Second Floor"], "kinds": ["wall", "room", "opening", "slab"]},
       "after": [{"part": "ground", "milestone": "walls"}], "instructions": "..."},
      {"key": "furnish", "name": "Furnishing", "role": "Furniture on both floors",
       "scope": {"kinds": ["asset"]},
       "after": [{"part": "ground", "milestone": "done"}, {"part": "upper", "milestone": "done"}],
       "instructions": "..."}]}
   ```

   The answer has, per part, a `handle` ("sw-1f0c9a4e7b2d3c58-ground") and a
   `url`, plus `how_to` text for the sub-agents.
3. Start one sub-agent per part, all at once (one Task each). Give each:
   - its handle, to pass as `agent` on every Guhit tool call;
   - its part's instructions, levels and kinds;
   - the `how_to` text and the sub-agent steps below.
   An agent in its own process (Codex, another terminal) uses the part's
   `url` as its MCP URL instead of the `agent` argument.
4. Watch: `swarm` action "wait" (no `for`) waits until every part is done,
   60 s per call; call it again until `reached` is true. `swarm` action
   "status" shows each part's state, milestones and what it drew.
5. Check the whole draft yourself: `look` mode "plan" per level with show
   dims and ids, `measure` key dimensions against the brief, `check`.
6. `swarm` action "finish" with a one-line summary. The draft is now ready
   for review.
7. Tell the user what was drawn, with the numbers the tools returned, and
   ask them to review the draft in Guhit Studio and press Accept (all of it,
   or part by part) or Discard. Never claim it is accepted. `swarm` action
   "accept" is always refused `people_only`.
8. Discard only when the user asks: `swarm` action "discard" with
   `force: true`.

## Steps for a sub-agent

- Pass `agent: "<your handle>"` on EVERY Guhit tool call. A call without it
  is not part of the swarm: it writes nothing (the model is locked) and it
  reads the model, not the draft.
- If your part comes after another, call `swarm` action "wait" first, with
  your handle and no `for`. It answers when what you wait for is reached;
  call it again on `timed_out`.
- Draw only your part's levels and kinds. Anything else is refused
  `out_of_scope`. One `build` per level works well; `dry_run` first for a
  big one.
- You read the draft, so the other parts' work is there: `query`, `look`
  mode "plan", `measure`, `check`.
- Report each milestone the moment you reach it: `swarm` action
  "milestone" with `name` ("walls" once your walls are closed). The parts
  waiting on it start then.
- Check your work with `look` mode "plan" and `measure` before you finish.
  `history` undo takes back only your own draft steps.
- End with `swarm` action "finish" and a one-line `summary`, then stop and
  report to the lead.

## Refusals and what they mean

| Code | Meaning | Do |
|---|---|---|
| `out_of_scope` | Outside your part's levels or kinds | Draw only your part; another part draws the rest |
| `draft_view` | 3D, renders, exports, tour videos and versions show the model, not the draft | Use `look` (plan, elevation, section, sheet, schedule), `query`, `measure`, `check` |
| `swarm_running` | A draft is open and the model waits for it | A person accepts or discards; a sub-agent forgot its handle |
| `draft_open` | Another swarm's draft is open | The user accepts or discards it first |
| `part_stopped` | The user stopped your part, or the swarm ended | Stop and tell the user |
| `people_only` | Only a person accepts | Ask the user to press Accept in Guhit Studio |
| `needs_force` | Discarding throws the draft away | Ask the user; call again with `force: true` only if they agree |
| `live_session` | Swarms do not run in a live session | Tell the user; they leave the session first |

## Honesty

Review items in a draft are suggestions to check, as anywhere in Guhit:
never code compliance, structural certification or permit approval. Report
the areas, lengths and counts the tools return; never estimate them.
