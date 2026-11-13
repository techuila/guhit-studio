# Trademarks

The Guhit Studio code and every other file in this repository are free software under the AGPL-3.0 (see [LICENSE](LICENSE)). The AGPL covers copyright. It grants no rights under trademark law, which section 7(e) of the AGPL allows, so it does not let you use the project's name or logo as the brand of your own product. This page is about trademark use only. It takes nothing away from the rights the AGPL gives you to copy, change and share the files.

It follows the spirit of the Firefox and Rust trademark policies: the code is yours to fork, the identity is not.

## What is covered

- The names **Guhit** and **Guhit Studio**, and names that look or sound like them.
- The Guhit logo mark and wordmark, the app icon, and the website's brand images, when used as marks.
- The project's domains, repository name, package and app identifiers (for example `ph.guhit.studio`), and social handles.

The files that show the marks are:

| Where | What |
|---|---|
| `assets/brand/` | Logo mark, wordmark, icon source, preview |
| `src-tauri/icons/` | App icons for macOS and Windows |
| `public/guhit.svg`, `public/favicon.svg` | The mark as used in the app |
| `site/favicon.svg`, `site/og.png`, `site/assets/logo-mark.svg`, `site/assets/wordmark.svg`, `site/assets/icon-256.png` | The mark as used on the website |

These files are in the repository because the official app builds from them. Like every other file there, they are under the AGPL: you may copy, change and share them, and keep them in a build you make for yourself. What this page restricts is using them, or anything close to them, as the brand of a different product (see below). A fork that ships builds replaces them with its own.

## What you may do without asking

- Build the app from source and run it for yourself or your team.
- Say that your project is **based on**, **a fork of** or **compatible with** Guhit Studio, as long as it is clearly your own product. For example: "Acme Plans, a fork of Guhit Studio".
- Write about the project, review it, link to it, and use the name to refer to it truthfully.
- Say that your software reads or writes `.guhit` files or talks to Guhit Studio over MCP.
- Package and redistribute an **unmodified** official release (for a package manager or a mirror), keeping the name and noting where it came from. Packaging changes that only make it run on your platform (paths, dependencies, build flags) are fine too. If you change anything else, rename it.

## What you may not do

- Use "Guhit" or "Guhit Studio" as the name of a fork, a product, a company, an app, a domain, a repository or a social handle.
- Use a name that could be mistaken for ours, such as Guhit Pro, Guhit Plus, Guhit Studio 2 or GuhitCAD.
- Use the logo, the app icon or the brand images, or anything close to them, as the mark of your own project.
- Suggest that the project, its maintainers or Aliteo made, endorse or support your product.
- Distribute modified builds under the Guhit name.

## If you fork

A fork must rename itself and change everything that points back to us before it ships a build:

1. **Name and icons.** Pick a new product name. Replace the logo and icon files listed above, `productName`, `publisher` and the window title in `src-tauri/tauri.conf.json`, and the visible name in `index.html`, the site and the README.
2. **App identifier.** Change `identifier` (`ph.guhit.studio`) in `src-tauri/tauri.conf.json`. The identifier decides where the app keeps its data, so a build with ours would share a folder with the official app.
3. **Updates.** Generate your own signing key with `pnpm tauri signer generate`. Put its public half in `plugins.updater.pubkey` and your release address in `plugins.updater.endpoints`. Without this, a fork keeps asking the official release feed for updates. It cannot install them (they are signed with a key only the project holds), and your users would hear about our releases instead of yours.
4. **Relay.** Live sessions over the internet use a relay whose address is built in from the `GUHIT_RELAY_URL` repository variable. Run your own (see `docs/RELAY.md`) or leave it unset.
5. **Source offer.** The AGPL requires you to give the users of your build, including users of a hosted copy, the source of your version. Keep the license, `NOTICE` and `THIRD_PARTY_LICENSES.md` with it.

## Questions

If you are unsure whether a use is fine, or you want permission for something this page does not allow, open an issue on the repository titled "Trademark question" before you ship it.

## Status

The names and logo are used in trade by the project owner. They are not registered trademarks today, and this page does not claim that they are. The owner may register them later. This page may change; the version in the repository's default branch is the one that applies.
