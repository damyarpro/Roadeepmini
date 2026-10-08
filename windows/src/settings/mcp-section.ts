// Settings → MCP: the app's local Roadeep MCP server, and the coding apps it is
// registered with (Claude Code, Cursor, VS Code, Codex…). Same contract as the
// hooks section in main.ts, one app at a time: diff first, dated backup, write
// only on an explicit click.

import "./mcp-clients.css";
import { Bridge } from "../core/bridge";
import { BridgeMcp, type LegacyCleanupReport, type McpClient, type McpPreview, type McpStatus } from "../core/bridge-mcp";
import { isolate, t } from "../core/i18n";
import { localizeError } from "../core/error-text";
import { h, clear } from "../views/dom";
import { CMD_SLOT, icon, sectionHead, statusBadge, withCommand, helpDisclosure } from "./ui";

function renderDiff(text: string): HTMLElement {
  const box = h("div", { class: "diff", dir: "ltr" });
  for (const line of text.split("\n")) {
    const cls = line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx";
    box.append(h("div", { class: cls, text: line }));
  }
  return box;
}

function pathRow(label: string, path: string, badge?: HTMLElement | null): HTMLElement {
  return h("div", { class: "path-row" },
    h("div", { class: "path-label" }, h("span", { dir: "auto", text: label }), badge ?? null),
    h("div", { class: "path", dir: "ltr", text: path || "—" }),
  );
}

function errorText(err: unknown): string {
  return String(err).replace(/^Error:\s*/, "");
}

/**
 * A refusal from Rust, localized. Some (JSON with comments, a YAML shape the
 * editor won't touch) carry the entry to paste by hand after the first line:
 * that part is machine text and gets its own LTR box.
 */
function errorBlock(err: unknown, wrap?: (text: string) => string): HTMLElement[] {
  const text = localizeError(err);
  const cut = text.indexOf("\n");
  const message = cut < 0 ? text : text.slice(0, cut);
  const manual = cut < 0 ? "" : text.slice(cut + 1).trim();
  const out = [h("div", { class: "notice err", dir: "auto", role: "alert", text: wrap ? wrap(message) : message })];
  if (manual) {
    out.push(h("div", { class: "mcp-manual" },
      h("span", { class: "set-hint", text: t("mcp.manualTitle") }),
      h("pre", { class: "diff", dir: "ltr", text: manual }),
    ));
  }
  return out;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

function cleanupResult(report: LegacyCleanupReport): HTMLElement {
  if (report.referenced) return h("div", { class: "notice warn", role: "status", text: t("mcp.legacyReferenced") });
  if (!report.removed.length && !report.kept.length) return h("div", { class: "notice ok", role: "status", text: t("mcp.legacyNone") });
  const list = (title: string, paths: string[]) => paths.length
    ? [h("strong", { text: title }), ...paths.map((p) => h("div", { class: "path", dir: "ltr", text: p }))]
    : [];
  return h("div", { class: `notice ${report.kept.length ? "warn" : "ok"}`, role: "status" },
    ...list(t("mcp.legacyRemoved"), report.removed),
    ...list(t("mcp.legacyKept"), report.kept),
  );
}

/** Explicit, settings-only removal of the previous version's relay folders. */
function legacyCleanupRow(): HTMLElement {
  const result = h("div", { class: "mcp-feedback" });
  const button = h("button", { type: "button", class: "sm", text: t("mcp.legacyCleanup") }) as HTMLButtonElement;
  button.addEventListener("click", async () => {
    if (button.disabled) return;
    button.disabled = true;
    clear(result);
    result.append(h("p", { class: "hint", role: "status", text: t("mcp.legacyCleanupBusy") }));
    try {
      const report = await BridgeMcp.legacyCleanup();
      clear(result);
      result.append(cleanupResult(report));
    } catch (err) {
      void Bridge.log(`legacy relay cleanup failed: ${errorText(err).split("\n", 1)[0]}`);
      clear(result);
      result.append(h("div", { class: "notice err", role: "alert", dir: "auto", text: t("mcp.legacyFailed", { err: isolate(localizeError(err)) }) }));
    } finally {
      button.disabled = false;
    }
  });
  return h("div", { class: "mcp-legacy" },
    h("div", { class: "actions" }, button),
    h("p", { class: "hint", text: t("mcp.legacyCleanupHint") }),
    result,
  );
}

function clientBadge(c: McpClient): HTMLElement {
  if (c.installed) return statusBadge("ok", t("mcp.state.connected"));
  if (c.legacyRelay) return statusBadge("warn", t("mcp.state.legacy"));
  if (c.conflict) return statusBadge("warn", t("mcp.state.conflict"));
  return statusBadge("off", t(c.detected ? "mcp.state.notConnected" : "mcp.state.notFound"));
}

export function mcpSection(): HTMLElement {
  const badgeSlot = h("span", { class: "sec-badge" });
  const serverCard = h("div", { class: "card" });
  const appsCard = h("div", { class: "group card mcp-apps", role: "group", "aria-labelledby": "grp-mcp-apps" });
  appsCard.hidden = true;
  const section = h("section", { class: "sec", "aria-labelledby": "sec-mcp-title" },
    sectionHead({ id: "sec-mcp-title", icon: "mcp", title: t("mcp.title"), desc: t("mcp.desc"), badge: badgeSlot }),
    serverCard,
    appsCard,
  );
  let status: McpStatus | null = null;
  // Survives redraws, so finishing an install under "Other apps" doesn't fold it away.
  let othersOpen = false;

  const setHead = () => {
    clear(badgeSlot);
    if (!status) return;
    const count = status.clients.filter((c) => c.installed).length;
    badgeSlot.append(count > 0
      ? statusBadge("ok", t("mcp.connectedCount", { count }))
      : statusBadge("off", t("status.notInstalled")));
  };

  const refresh = async () => {
    status = await BridgeMcp.status();
    setHead();
    drawServer();
    drawApps();
  };

  function drawServer() {
    clear(serverCard);
    if (!status) {
      serverCard.append(h("p", { class: "hint", text: t("mcp.unavailable") }));
      return;
    }
    const anyInstalled = status.clients.some((c) => c.installed);
    serverCard.append(
      h("div", { class: "hint-stack" },
        h("p", { class: "hint", text: t(anyInstalled ? "mcp.installedHint" : "mcp.notInstalledHint") }),
        helpDisclosure(`${t("mcp.tools")}\n\n${t("mcp.costNote")}`, `${t("settings.help")}: ${t("mcp.title")}`),
      ),
      h("div", { class: "kv-list" },
        pathRow(t("mcp.server"), status.exePath,
          statusBadge(status.exeReady ? "ok" : "err", t(status.exeReady ? "status.ready" : "status.missing"))),
        h("div", { class: "path-row" },
          h("div", { class: "path-label" },
            h("span", { text: t("mcp.account") }),
            statusBadge(status.signedIn ? "ok" : "off", t(status.signedIn ? "status.connected" : "status.signedOut")),
          ),
          status.signedIn ? null : h("p", { class: "hint", text: t("mcp.signedOut") }),
        ),
      ),
    );
    if (!status.exeReady) {
      serverCard.append(h("div", {
        class: "notice warn",
        }, ...withCommand(t("mcp.exeMissing", { cmd: CMD_SLOT }), "cargo build --release -p roadeep-mcp")));
    }
    serverCard.append(legacyCleanupRow());
  }

  function appsHead(): HTMLElement {
    return h("div", { class: "subhead" }, h("h3", { id: "grp-mcp-apps", tabindex: "-1", text: t("mcp.appsTitle") }));
  }

  function clientRow(c: McpClient, exeReady: boolean): HTMLElement {
    const actions = h("div", { class: "mcp-client-actions" });
    const button = (text: string, label: string, install: boolean, cls?: string) => {
      const b = h("button", {
        type: "button", class: cls ? `sm ${cls}` : "sm", text, "aria-label": label, title: label,
        onclick: () => void showPreview(c, install),
      }) as HTMLButtonElement;
      // An entry pointing at a server that isn't there gives the app a broken
      // MCP server on every start.
      if (install && !exeReady) {
        b.disabled = true;
        b.title = t("mcp.exeNotInstalled");
      }
      return b;
    };
    if (c.legacyRelay && !c.installed) {
      actions.append(button(t("mcp.update"), t("mcp.updateIn", { app: c.name }), true, "primary"));
    } else if (c.installed) {
      actions.append(
        button(t("mcp.update"), t("mcp.updateIn", { app: c.name }), true),
        button(t("mcp.remove"), t("mcp.removeFrom", { app: c.name }), false, "danger"),
      );
    } else {
      actions.append(button(t("mcp.add"), t("mcp.addTo", { app: c.name }), true));
    }
    return h("li", { class: "mcp-client" },
      h("div", { class: "mcp-client-text" },
        h("div", { class: "mcp-client-title" },
          h("span", { class: "mcp-client-name", dir: "ltr", text: c.name }),
          clientBadge(c),
        ),
        h("span", { class: "mcp-client-path", dir: "ltr", title: c.configPath, text: c.configPath }),
      ),
      actions,
    );
  }

  function drawApps() {
    clear(appsCard);
    appsCard.hidden = !status;
    if (!status) return;
    const detected = status.clients.filter((c) => c.detected);
    const others = status.clients.filter((c) => !c.detected);
    appsCard.append(appsHead(), helpDisclosure(t("mcp.appsHint"), `${t("settings.help")}: ${t("mcp.title")}`));

    if (detected.length > 0) {
      const list = h("ul", { class: "mcp-client-list" });
      for (const c of detected) list.append(clientRow(c, status.exeReady));
      appsCard.append(list);
    } else {
      appsCard.append(h("p", { class: "hint", text: t("mcp.noneDetected") }));
    }

    if (others.length > 0) {
      const list = h("ul", { class: "mcp-client-list" });
      for (const c of others) list.append(clientRow(c, status.exeReady));
      const more = h("details", { class: "mcp-others" },
        h("summary", {},
          h("span", { class: "chev", "aria-hidden": "true" }, icon("chevronDown", 16)),
          h("span", { text: t("mcp.otherApps", { count: others.length }) }),
        ),
        helpDisclosure(t("mcp.otherAppsHint"), `${t("settings.help")}: ${t("mcp.title")}`),
        list,
      );
      more.open = othersOpen || detected.length === 0;
      more.addEventListener("toggle", () => { othersOpen = more.open; });
      appsCard.append(more);
    }
  }

  function backToList() {
    drawApps();
    appsCard.querySelector<HTMLElement>("#grp-mcp-apps")?.focus({ preventScroll: true });
  }

  /** The review step for one app, in place of the list. */
  function flowHead(c: McpClient): HTMLElement {
    const title = h("h3", { id: "grp-mcp-apps", tabindex: "-1", dir: "ltr", text: c.name });
    queueMicrotask(() => {
      title.focus({ preventScroll: true });
      title.scrollIntoView({ block: "nearest", behavior: "auto" });
    });
    return h("div", { class: "subhead" }, title, clientBadge(c));
  }

  async function showPreview(c: McpClient, install: boolean) {
    let preview: McpPreview;
    try {
      preview = await BridgeMcp.preview(c.id, install);
    } catch (err) {
      // An unreadable or uneditable config stops here rather than being written over.
      void Bridge.log(`mcp preview failed (${c.id}): ${errorText(err).split("\n", 1)[0]}`);
      clear(appsCard);
      appsCard.append(
        flowHead(c),
        ...errorBlock(err),
        h("div", { class: "actions" }, h("button", { type: "button", text: t("common.back"), onclick: backToList })),
      );
      return;
    }
    const file = isolate(fileName(preview.configPath));
    clear(appsCard);
    appsCard.append(flowHead(c), h("p", { class: "hint", text: t(install ? "mcp.previewInstall" : "mcp.previewRemove", { file }) }));
    if (install && c.conflict) {
      appsCard.append(h("div", { class: "notice warn", text: t("mcp.conflict", { app: isolate(c.name) }) }));
    }
    appsCard.append(
      renderDiff(localizeError(preview.diff)),
      h("div", { class: "kv-list" },
        pathRow(t("mcp.configFile"), preview.configPath),
        pathRow(t("mcp.backup"), preview.backup),
      ),
    );
    const feedback = h("div", { class: "mcp-feedback" });
    const confirm = h("button", {
      type: "button",
      class: install ? "primary" : "danger",
      text: t(install ? "mcp.confirmWrite" : "mcp.confirmRemove"),
    }) as HTMLButtonElement;
    confirm.addEventListener("click", async () => {
      confirm.disabled = true;
      clear(feedback);
      try {
        const backup = await BridgeMcp.apply(c.id, install, preview.fingerprint);
        clear(appsCard);
        appsCard.append(flowHead(c), h("div", {
          class: "notice ok",
          role: "status",
          text: backup
            ? t(install ? "mcp.doneInstall" : "mcp.doneRemove", { backup: isolate(backup), app: isolate(c.name) })
            : t(install ? "mcp.doneInstallNoBackup" : "mcp.doneRemoveNoBackup", { app: isolate(c.name) }),
        }));
        window.setTimeout(() => void refresh(), 2600);
      } catch (err) {
        confirm.disabled = false;
        void Bridge.log(`mcp apply failed (${c.id}): ${errorText(err).split("\n", 1)[0]}`);
        feedback.append(...errorBlock(err, (msg) => t("mcp.writeFailed", { err: isolate(msg) })));
      }
    });
    appsCard.append(
      feedback,
      h("div", { class: "actions" }, confirm, h("button", { type: "button", text: t("common.cancel"), onclick: backToList })),
    );
  }

  serverCard.append(h("p", { class: "hint", text: t("mcp.loading") }));
  void refresh();
  return section;
}
