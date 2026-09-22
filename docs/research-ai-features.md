# Research: powerful copilot, AI renders, 3D quality, house-tour videos

Requested 2026-09-22. Costs checked the same day. Nothing here is built yet; see the decision table at the end.

## 1. Copilot with PH architectural context

### What exists in the app today
Claude Opus 5 with 21 strict tools, staged batches, ghost preview, user approval. No reference documents. The system prompt forbids compliance claims (research section 5.3, DECISIONS D8).

### Sources to ground it in
| Document | Status | Where |
|---|---|---|
| PD 1096 National Building Code + 2004 IRR (room sizes, ceiling heights, setbacks, light and ventilation, stairs, exits) | Public law. Official scanned PDFs exist; VIZCODE hosts a cleaned digital text | vizcodeph.com/code-library |
| BP 344 Accessibility Law + IRR (ramps, doors, toilets, parking) | Public law | vizcodeph.com, DPWH |
| RA 9514 Fire Code + 2019 IRR (occupancy, exits, travel distance) | Public law | BFP |
| Philippine Green Building Code 2015 (referral code of PD 1096) | Public PDF | DPWH, iibh.org mirror |
| NSCP 2015 (structural) | Copyrighted by ASEP, not free | Cite by section only, do not embed |
| RA 9266 Architecture Act (who may sign and seal) | Public law | PRC |
| Local zoning ordinances (Quezon City, Cebu, Davao) | Per LGU, changes often | Later, per city |

Key numbers the copilot should be able to cite from PD 1096 IRR Rule 7 and 8: habitable room minimum 6.00 m2 with a 2.00 m least dimension, kitchen 3.00 m2 / 1.50 m, bath 1.20 m2 / 0.90 m, ceiling 2.40 m with artificial ventilation and 2.70 m with natural ventilation, window area at least 10 percent of floor area for natural light, door widths, stair rise and run, setbacks by road right of way.

### How to build it (recommended)
1. Store the public code texts as PDFs in the app bundle, sectioned. Send the relevant sections as `document` blocks with `citations: {enabled: true}`; every answer then carries the cited text and page. Cache the documents with prompt caching (1 h TTL) so the cost is paid once per session, not per message.
2. Add a `check_against_code` tool set that runs the deterministic checks the engine already has (room area, least dimension, door width, window to floor ratio) against the numbers in the IRR, and returns findings with the IRR section. The model explains, the engine computes. This matches Appendix B of the research: "answers computed from model data, not estimated".
3. Keep the wording as "reference" and "suggestion to verify with the building official". Never "compliant".

### Cost per copilot turn (Claude API list prices, 2026-06)
| Item | Opus 5 | Sonnet 5 |
|---|---|---|
| Input | $5.00 per 1M tokens | $2.00 |
| Output | $25.00 per 1M | $10.00 |
| Cache write | 1.25x input | 1.25x |
| Cache read | about 0.1x input | 0.1x |

A typical edit turn today: about 6k input (prompt, tools, project context), 400 output, 2 tool rounds: about $0.07 on Opus 5, $0.03 on Sonnet 5. With 60k tokens of code text cached: first turn of a session about +$0.38 (Opus) then about +$0.03 per turn from cache. A student doing 40 turns a day costs about $3 to $4 a day on Opus, $1.50 on Sonnet. The research's Solo plan is PHP 499 to 799 a month (about $9 to $14), so AI credits must be metered (research section 12: "keep drafting independent of AI credits").

## 2. AI presentation renders (Tier 2, DECISIONS D9 open)

Tier 1 (deterministic 3D capture tied to revision and camera) is done. Tier 2 conditions a generative image on that capture. Higgsfield is the connector already attached to this Claude session, so its models and prices were checked live.

### Higgsfield: two different products
| | Consumer plan + MCP connector (what is attached here) | Higgsfield API (for the app) |
|---|---|---|
| Who pays | Axl's own account (Max plan, 990 credits today) | The app, USD balance, pay as you go, no subscription |
| Use in a product for other users | No. It is a personal account | Yes. "Content generated through the API can be used in commercial products" |
| Auth | OAuth connector | API key, 20 concurrent requests |
| Pattern | tool call, widget | REST or SDK, async job id then poll |

So the MCP is good for prototyping prompts and styles right now, and useless as the production path. The app needs the API.

### Measured costs (connector credits, preflight with get_cost, nothing generated)
| Job | Model | Credits | About USD at plan rate (about $0.05 per credit) |
|---|---|---|---|
| Image to image 2k, 16:9 | Nano Banana Pro (Google) | 2 | $0.10 |
| Image to image 2k high | GPT Image 2.5 (OpenAI) | 3 | $0.15 |
| Image to video 5 s, standard, silent | Kling 3.0 | 7.5 | $0.37 |
| Image to video 10 s, pro, silent | Kling 3.0 | 17.5 | $0.87 |
| Video 10 s, 1080p, silent | Seedance 2.5 | 120 | $6.00 |

Published API rates (USD, higgsfield.ai/blog/higgsfield-api): images $0.0032 to $0.0059 each, video $0.042 to $0.20+ per second, a 10 s Kling 3.0 clip $1.12.

### Direct providers, for comparison
The image-to-image job can also be bought straight from Google (Gemini image, the model behind "Nano Banana") or OpenAI (GPT Image) at similar per-image prices, one provider fewer. The abstraction in the app should be provider-neutral: an `ImageProvider` trait with `render(view_png, depth_png, prompt, style) -> png`. Higgsfield first because it fronts several models with one key, direct providers later if the unit price matters.

### Quality note
Model view + prompt keeps composition but not geometry exactly. To hold the walls, openings and roof line, send a depth or edge image as a second reference where the model accepts references (Nano Banana Pro and GPT Image 2.5 both take `image_references`). This is the "geometry as conditioning" rule from research section 10, and it is why the 3D captures now save the camera and revision.

## 3. Better 3D (Blender or not)

| Option | Quality | Cost | Fit |
|---|---|---|---|
| Improve the in-app three.js view: environment map lighting, ambient occlusion, soft shadows, better materials, a sky model | Good massing model, not photoreal | Engineering only, no runtime cost | Do first. It is what the client sees live and what the AI render is conditioned on |
| Browser path tracer (three-gpu-pathtracer, WebGPU) for a "still render" button | Near photoreal for simple scenes, 10 to 60 s per frame | Engineering only, runs on the user's GPU | Good second step, keeps everything offline |
| Blender headless (Cycles or Eevee) as an optional local renderer: export glTF, drive `blender -b` with a Python script | Photoreal, sun/sky, real materials | Free software, 300 MB install the user does, minutes per frame on a laptop | Optional "Pro render" for users who have Blender. Not a dependency of the app |
| Blender in the cloud (render farm or a GPU VPS) | Same as above, fast | Farms bill per GPU minute; a GPU VPS is about $1 to $3 per hour | Only if demand is proven |
| AI render (section 2) | Beautiful, not accurate | $0.10 to $0.15 per image | Client visuals, labelled as visualization |

Recommendation: three.js quality pass now, glTF export now (it is also useful for handoff), Blender as an optional external renderer later, cloud never until unit economics say so.

## 4. House-tour videos

Two routes, and they are not the same product.

| Route | What it is | Accuracy | Cost per 30 s tour |
|---|---|---|---|
| A. Deterministic fly-through recorded from the live 3D view | Camera path through saved Camera elements, rendered frame by frame in-app to MP4 (WebCodecs in the webview) | Exact, matches the plan | $0, runs locally |
| B. AI video from stills | Take 3 to 6 captures along the path, animate each with Kling 3.0 image-to-video (5 to 10 s), stitch | Pretty, but walls and furniture drift between clips | 4 clips x 10 s pro: about $3.50 in credits, about $4.50 at API rates |
| A + B | Route A for the walkthrough, one AI-stylized hero clip for the intro | Best of both | about $1 |

Recommendation: build route A first (it is deterministic, offline, and free), then offer route B as a paid "AI clip" with the visualization label.

## 5. Costed decisions for Axl

| Question | Options | Recommendation |
|---|---|---|
| Codes knowledge | PDFs with citations + engine checks (recommended); or a hosted RAG service | Embed PD 1096 IRR, BP 344, Fire Code IRR, Green Building Code sections with citations. About $0.03 per cached turn. |
| Copilot model | Opus 5 for edits, Sonnet 5 for questions | Route by intent. Cuts cost about half. |
| Render provider | Higgsfield API (one key, many models) vs Google/OpenAI direct | Higgsfield API first. Budget $0.15 per image. Requires an API account and USD top-up. |
| 3D quality | three.js pass, then browser path tracer, Blender optional | Yes to the first two. |
| Video | In-app fly-through, then optional AI clips | Yes, fly-through first. |
| Pricing to users | AI credits separate from the plan (research section 12) | Meter renders and videos per unit; give the copilot a monthly token allowance. |

## Sources
- Higgsfield API: https://higgsfield.ai/blog/higgsfield-api
- Higgsfield plan pricing: https://www.scopeful.org/blog/higgsfield-pricing-2026 and https://www.blotato.com/blog/higgsfield-pricing
- Claude pricing: claude-api skill model table, cached 2026-06-24
- PD 1096 text: https://vizcodeph.com/code-library/pd-1096-national-building-code-of-the-philippines/ and https://www.architectureboard.com.ph/wp-content/uploads/2019/09/1.11-PD1096-1977NBCP_highlights.pdf
- Green Building Code PDF: https://www.iibh.org/kijun/pdf/Philippines_05_Green_Building_Code_of_Philippones_Y2015.pdf
- Blender headless rendering: https://github.com/oqton/blenderless and https://contabo.com/blog/gpu-rendering-on-a-vps-blender-cinema-4d-unreal-2026/
