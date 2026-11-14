# P2-M10 Claude Code connection

Status: implemented native workflow; signed and actual-client acceptance open.

Open Settings → Servers and choose **Check connection settings**. This does
not start a server or enroll Claude. With the supported Claude Code version
installed, choose **Connect Claude Code** explicitly. The app saves a user-scope
HTTP entry named `samsung-tv-remote` and grants MCP access afterward. Other
settings and project files are preserved. No token is copied into configuration.

The current compatibility preflight admits Claude Code **2.1.325**. That version
accepted the `headersHelper` entry and honored the writer's sibling
`.claude.json.lock` directory in disposable configuration tests on 2026-11-14.
Other versions require renewed compatibility evidence before being admitted.
The version command has a three-second deadline and bounded output; it does
not run a client helper or contact a model. This is writer compatibility,
not signed endpoint, model or actual-client tool acceptance.

In a separate isolated development smoke, actual `claude mcp get` and
`claude mcp list` reported Connected through the dynamic helper with the GUI
closed and fake server initially absent. The helper cold-started the server,
which was explicitly stopped afterward. No authenticated model session was
available in that fresh Claude configuration. This health result does not
establish production signing, Keychain access, model tool execution or native
Local Network permission behavior.

The GUI checks the user's standard Claude configuration, or an absolute
`CLAUDE_CONFIG_DIR` inherited by the GUI. A custom directory used only in a
Terminal session is not automatically shared with a normally launched app.
The helper executable comes from this application bundle, never PATH. The
Claude executable is searched at `~/.local/bin/claude`,
`/opt/homebrew/bin/claude` and `/usr/local/bin/claude` for its version probe.

Preflight rejects managed MCP/settings files, managed preference presence,
restrictive MCP settings and same-name local/project entries in known projects
and their ancestors. Malformed, duplicate-member, unsafe or unreadable user
configuration fails closed. This conservative preflight does not evaluate
remote organization policy or promise that a future/new project cannot add an
override. Claude's effective policy and actual attachment need their own review.
Managed deployments require an administrator-reviewed setup; the app does not
rewrite policy to make enrollment work. The client scope and dynamic-header
behavior are described in [Claude's MCP documentation](https://code.claude.com/docs/en/mcp)
and [managed MCP documentation](https://code.claude.com/docs/en/managed-mcp).

After Connect, **Verify MCP endpoint** invokes the destination-bound helper and
reads the exact shipped catalog. It never follows redirects or sends a TV
command. Verification can start an enrolled missing server but cannot clear an
intentional Stop. A successful app-side read is labeled separately from actual
Claude Code attachment, which is not inferred. Reopen the intended Claude
session and review its `/mcp` status for actual-client evidence.

After moving/updating the application, an entry with the same trusted endpoint
and a changed helper offers an explicit repair confirmation. Repair compares
the exact observed entry before replacing it. An entry with a different URL or
an unrecognized shape remains a conflict. Resolve it explicitly, then recheck;
the app never silently replaces it.

**Disconnect Claude Code** affects all Claude sessions, fences/revokes access
before removing the unchanged entry and preserves already admitted effects.
The GUI remains usable. A failed removal keeps acknowledged disabled access;
an unknown revocation is reported as unconfirmed. Retry uses the same original
generation/key and expected entry. **Check connection settings** does not
restore enrollment. Explicit Connect after disable obtains a new bearer under
the same private principal. Server Stop still requires explicit Resume.

The development probe refuses connection management unless
`CLAUDE_CONFIG_DIR` is explicitly set to an isolated test directory. Automated
fixtures must not edit the owner's configuration, use production storage or
copy credentials into logs. See the [acceptance topology](milestone-10-architecture.md#acceptance-topology)
for the distinct fake-port, production-helper and native-permission lanes.
