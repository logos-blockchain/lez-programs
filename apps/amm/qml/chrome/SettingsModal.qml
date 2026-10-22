pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import "../components/liquidity"

// App settings modal, opened from the cogwheel in Main.qml. Currently a single
// "Registry" section: the known-tokens / known-pools registry URL and the network
// picker, bound to the AMM backend (registryUrl / saveRegistryUrl / networks /
// activeNetwork / selectNetwork). This is AMM-specific, so it lives in the app
// rather than the shared wallet UI.
Popup {
    id: root

    property var backend: null

    AmmTheme { id: theme }

    parent: Overlay.overlay
    modal: true
    focus: true
    width: parent && parent.width > 32 ? Math.max(0, Math.min(440, parent.width - 32)) : 300
    x: parent ? Math.round((parent.width - width) / 2) : 0
    y: parent ? Math.round((parent.height - height) / 2) : 0
    padding: 20
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    onOpened: {
        registryUrlField.text = root.backend ? (root.backend.registryUrl || "") : ""
        networkSelector.syncSelection()
    }

    Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.4) }

    background: Rectangle {
        radius: 16
        color: theme.colors.cardBg
        border.color: theme.colors.border
        border.width: 1
    }

    contentItem: ColumnLayout {
        spacing: 14

        RowLayout {
            Layout.fillWidth: true

            Label {
                Layout.fillWidth: true
                text: qsTr("Settings")
                color: theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 17
            }

            Label {
                text: "✕"  // close
                color: theme.colors.textSecondary
                font.pixelSize: 16
                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -8
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.close()
                }
            }
        }

        Label {
            text: qsTr("Registry")
            color: theme.colors.textPrimary
            font.bold: true
        }

        Label {
            Layout.fillWidth: true
            text: qsTr("Saved registry URL. This preference is not necessarily the snapshot on screen. Leave empty to configure none.")
            color: theme.colors.textSecondary
            font.pixelSize: 11
            wrapMode: Text.WordWrap
        }

        TextField {
            id: registryUrlField
            objectName: "settingsRegistryUrlField"
            Layout.fillWidth: true
            text: root.backend ? (root.backend.registryUrl || "") : ""
            placeholderText: qsTr("https://…/amm-registry.json")
            color: theme.colors.textPrimary
            background: Rectangle {
                radius: 8
                color: theme.colors.inputBg
                border.color: theme.colors.border
                border.width: 1
            }
        }

        Label {
            id: registryStatus
            objectName: "settingsRegistryStatus"
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: theme.colors.textSecondary
            font.pixelSize: 11
            text: {
                const backend = root.backend
                if (!backend)
                    return ""
                const source = backend.registrySnapshotSource || "none"
                const origin = backend.registrySnapshotOrigin || ""
                const requestUrl = backend.registryRequestUrl || ""
                const state = backend.registryRefreshState || "idle"
                const lines = []
                if (source === "local")
                    lines.push(qsTr("Lists are coming from local token files."))
                else if (source === "cache")
                    lines.push(qsTr("Lists are the cached snapshot from %1.").arg(origin))
                else if (source === "remote" && origin.length > 0)
                    lines.push(qsTr("Lists were loaded from %1.").arg(origin))
                else if (source === "remote")
                    lines.push(qsTr("Lists were loaded from a registry."))
                else
                    lines.push(qsTr("No registry snapshot is loaded."))
                if (backend.registryOverrideKind === "env_url")
                    lines.push(qsTr("AMM_REGISTRY_URL overrides the saved URL. Requesting %1.").arg(requestUrl))
                else if (backend.registryOverrideKind === "local_files")
                    lines.push(qsTr("Local files override the saved URL."))
                else if (state === "loading" && requestUrl.length > 0 && requestUrl !== origin)
                    lines.push(qsTr("Refreshing from %1.").arg(requestUrl))
                if (state === "loading")
                    lines.push(qsTr("Refresh in progress."))
                else if (state === "failed")
                    lines.push(qsTr("The last refresh failed: %1").arg(backend.registryRefreshError || qsTr("unknown error")))
                if (backend.registryNetworkNote)
                    lines.push(backend.registryNetworkNote)
                return lines.join("\n")
            }
        }

        Button {
            id: saveButton
            objectName: "settingsRegistrySaveButton"
            Layout.fillWidth: true
            text: qsTr("Save")
            onClicked: {
                if (root.backend)
                    root.backend.saveRegistryUrl(registryUrlField.text)
            }
            contentItem: Text {
                text: saveButton.text
                color: "#FFFFFF"
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }
            background: Rectangle {
                radius: 8
                implicitHeight: 36
                color: saveButton.pressed ? theme.colors.ctaPressedBg
                     : saveButton.hovered ? theme.colors.ctaHoverBg
                     : theme.colors.ctaBg
            }
        }

        Label {
            Layout.fillWidth: true
            visible: networkSelector.count > 0
            text: qsTr("Network")
            color: theme.colors.textSecondary
            font.pixelSize: 11
        }

        ComboBox {
            id: networkSelector
            objectName: "settingsNetworkSelector"
            Layout.fillWidth: true
            visible: count > 0
            textRole: "name"
            valueRole: "id"
            model: root.backend ? root.backend.networks : []

            // Select the active network (which defaults to the first), falling back
            // to the first item. Imperative — the model syncs from the backend after
            // this is created, so a currentIndex binding would compute -1 before the
            // model arrives and never re-run.
            function syncSelection() {
                if (!root.backend || count === 0)
                    return
                const i = indexOfValue(root.backend.activeNetwork)
                currentIndex = i >= 0 ? i : 0
            }
            Component.onCompleted: syncSelection()
            onCountChanged: syncSelection()
            Connections {
                target: root.backend
                function onActiveNetworkChanged() { networkSelector.syncSelection() }
            }

            onActivated: {
                if (root.backend)
                    root.backend.selectNetwork(currentValue)
            }
        }
    }
}
