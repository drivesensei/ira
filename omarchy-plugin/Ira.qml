import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// IRA — Omarchy bar widget and panel for the IRA terminal file manager
// (https://github.com/drivesensei/ira).
//
// The panel is what makes a stock Omarchy machine self-sufficient: if the
// `ira` binary is not on PATH it says so, and its install action runs
// omarchy-plugin/install.sh (which sits next to this file) in a floating
// terminal — the latest GitHub release lands in ~/.local/bin with no sudo and
// a launcher entry, so IRA also shows up in the Omarchy app menu.
//
// Interactions, following the bar's conventions:
//   left click   panel (actions + status)
//   right click  open IRA, or install it when it is missing
//
// The panel is keyboard-driven too: summon it with
//   omarchy-shell shell summon drivesensei.ira
// then Up/Down or j/k to move, Enter/Space to activate, Escape to close.
//
// IRA is launched through `omarchy-launch-or-focus-tui`, which gives the
// window the app id the rest of Omarchy expects for `ira` (org.omarchy.ira),
// so it is tagged as a terminal by the stock Hyprland rules and an existing
// IRA window is focused instead of duplicated.
Panel {
  id: root

  moduleName: "drivesensei.ira"
  ipcTarget: "drivesensei.ira"

  property bool iraPresent: false
  property string iraVersion: ""
  property int cursor: 0
  property bool cursorActive: false

  // install.sh ships inside the plugin checkout, next to this file.
  readonly property string installScript: {
    var resolved = String(Qt.resolvedUrl("install.sh"))
    return resolved.indexOf("file://") === 0 ? decodeURIComponent(resolved.substring(7)) : ""
  }

  readonly property string statusText: {
    if (!root.iraPresent) return "Not installed"
    return root.iraVersion !== "" ? root.iraVersion : "Installed"
  }

  readonly property var rows: {
    var out = []
    if (root.iraPresent) {
      out.push({
        "kind": "open",
        "label": "Open IRA",
        "detail": "Terminal file manager"
      })
      out.push({
        "kind": "install",
        "label": "Update IRA",
        "detail": "Re-install the latest GitHub release"
      })
    } else {
      out.push({
        "kind": "install",
        "label": "Install IRA",
        "detail": "Latest release → ~/.local/bin (no sudo)"
      })
    }
    return out
  }

  // ---------------------------------------------------------------- actions

  function run(command) {
    if (root.bar && typeof root.bar.run === "function") root.bar.run(command)
    else Util.execDetached(command)
  }

  function launchIra() {
    root.close()
    root.run("omarchy-launch-or-focus-tui ira")
  }

  function installIra() {
    if (!root.installScript) return
    root.close()
    root.run("omarchy-launch-floating-terminal-with-presentation "
      + Util.shellQuote("bash " + Util.shellQuote(root.installScript)))
    // The install runs in its own terminal, so the only completion signal we
    // get is the binary appearing on PATH; watch for it and re-probe.
    if (!iraWatcher.running) iraWatcher.running = true
  }

  function activateAt(index) {
    var row = root.rows[index]
    if (!row) return
    if (row.kind === "open") root.launchIra()
    else if (row.kind === "install") root.installIra()
  }

  function moveCursor(dy) {
    if (root.rows.length === 0) return
    root.cursorActive = true
    var next = root.cursor + dy
    root.cursor = Math.max(0, Math.min(root.rows.length - 1, next))
  }

  function primaryAction() {
    if (root.iraPresent) root.launchIra()
    else root.installIra()
  }

  // ----------------------------------------------------------------- probes

  function reprobe() {
    if (!iraProbe.running) iraProbe.running = true
  }

  function applyProbe(text) {
    var lines = String(text || "").split("\n")
    var version = ""
    var found = false
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].trim()
      if (line === "") continue
      found = true
      if (line.indexOf("ira ") === 0) version = line
    }
    root.iraVersion = version
    root.iraPresent = found
    if (root.cursor >= root.rows.length) root.cursor = 0
  }

  onOpenedChanged: {
    if (!opened) return
    root.cursor = 0
    root.cursorActive = false
    root.reprobe()
  }

  Component.onCompleted: root.reprobe()

  Process {
    id: iraProbe
    command: ["bash", "-lc", "command -v ira 2>/dev/null && ira --version 2>/dev/null"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyProbe(text)
    }
  }

  Process {
    id: iraWatcher
    command: ["bash", "-lc", "for _ in $(seq 1 180); do command -v ira >/dev/null 2>&1 && exit 0; sleep 1; done; exit 1"]
    onExited: root.reprobe()
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "\uf07b"
    tooltipText: root.iraPresent
      ? "IRA\n" + root.statusText + "\n\nLeft click: panel · Right click: open"
      : "IRA\nNot installed\n\nLeft click: panel · Right click: install"

    onPressed: function(button) {
      if (button === Qt.RightButton) root.primaryAction()
      else root.toggle()
    }

    onTooltipHoveredChanged: {
      if (!root.bar) return
      if (tooltipHovered) root.bar.showTooltip(button, root.tooltipText)
      else root.bar.hideTooltip(button)
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(320))
    contentHeight: panel.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function(dx, dy) { root.moveCursor(dy) }
      onActivateRequested: root.activateAt(root.cursor)
      onCloseRequested: root.close()

      Column {
        id: column
        anchors.fill: parent
        spacing: Style.space(8)

        PanelHero {
          width: parent.width
          title: "IRA"
          detail: root.iraPresent ? root.iraVersion.replace("ira ", "") : ""
          meta: root.iraPresent ? "Terminal file manager" : "Not installed"
          foreground: root.barForeground
          fontFamily: Style.font.family
          iconComponent: Component {
            Text {
              text: "\uf07b"
              color: root.barForeground
              font.family: Style.font.family
              font.pixelSize: Style.font.display
            }
          }
        }

        PanelSeparator { foreground: root.barForeground }

        Repeater {
          model: root.rows

          delegate: CursorSurface {
            id: row
            required property var modelData
            required property int index

            width: column.width
            implicitHeight: rowLabels.implicitHeight + Style.space(12)
            hasCursor: root.cursorActive && index === root.cursor
            foreground: root.barForeground
            accent: Color.accent

            Column {
              id: rowLabels
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.leftMargin: Style.space(10)
              anchors.rightMargin: Style.space(10)
              anchors.verticalCenter: parent.verticalCenter
              spacing: Style.space(1)

              Text {
                textFormat: Text.PlainText
                width: parent.width
                text: row.modelData.label
                color: root.barForeground
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                elide: Text.ElideRight
              }

              Text {
                textFormat: Text.PlainText
                width: parent.width
                text: row.modelData.detail
                color: Qt.darker(root.barForeground, 1.4)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
                elide: Text.ElideRight
              }
            }

            MouseArea {
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onEntered: {
                root.cursor = row.index
                root.cursorActive = true
              }
              onClicked: root.activateAt(row.index)
            }
          }
        }
      }
    }
  }
}
