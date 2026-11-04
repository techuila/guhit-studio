# Security policy

## Report a vulnerability

Please report it privately. Do not open a public issue, a pull request or a discussion for a vulnerability.

1. Open the repository's **Security** tab and choose **Report a vulnerability**, or go straight to <https://github.com/techuila/guhit-studio/security/advisories/new>.
2. Say what is affected (the desktop app, the MCP server, live sessions or the relay, the updater, a file import), the version, how to reproduce it, and what an attacker gains.
3. A proof of concept helps. Do not attach other people's private projects or any real API keys.

You will get a reply from a maintainer. We aim to acknowledge a report within 7 days, tell you whether we accept it, and keep you posted until a fix ships. If you want credit in the release notes, say so; if you want to stay anonymous, say that instead.

Please give us a reasonable time to fix the problem before you publish details. We will not ask you to wait forever: if a fix is not possible, we will say so and agree a date with you.

## Supported versions

Only the latest release gets security fixes. The app updates itself, so most people are on it within a day.

## Scope

In scope:

- The desktop app and the data it stores (projects, versions, the saved API keys).
- The MCP server (`/mcp` on 127.0.0.1 and the stdio mode) and anything that lets a web page or another program reach it.
- Live sessions: the invite, the encrypted connection, and the relay in `crates/guhit-relay`.
- The updater and the release pipeline (signing, `latest.json`, the release workflow).
- Parsing of files the app opens: `.guhit` bundles, DXF and DWG, glTF, OBJ.

Out of scope:

- The dev bridge (`crates/guhit-devbridge`). It exists for development, listens on localhost only and is not part of the releases.
- Anything that needs an attacker who already controls the person's computer or account.
- Wrong or unsafe answers from an AI model. AI output is labelled as a suggestion in the app; report a problem in how the app handles it, not in the model.
- A vulnerability in a dependency with no way to reach it from Guhit Studio. Report that to the dependency. If you can show that it affects us, report it here too.
- Denial of service by sending a very large number of requests to a server you do not run.

## Testing rules

- Test against your own copy of the app, your own project and your own relay. Do not test against other people's live sessions or against a relay you do not run.
- Stop when you can show the problem. Do not read, change or delete data that is not yours.

Good-faith research that follows these rules is welcome, and we will not take action against it.

## Where keys live

The copilot and AI rendering keys are stored in the app's data folder with user-only permissions and are sent only to the provider you chose. They are never saved in a project. See the Security section of the [README](README.md).
