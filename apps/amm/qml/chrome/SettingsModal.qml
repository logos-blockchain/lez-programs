pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import "../components/liquidity"

// The saved URL is editable independently of environment overrides and the
// provenance of the registry snapshot currently used by the app.
Popup {
    id: root

    property var backend: null
    readonly property var registryStatus: Object.assign({
        source: "none", effectiveUrl: "", snapshotUrl: "", override: "",
        loading: false, error: ""
    }, backend ? backend.registryStatus : {})

    AmmTheme { id: theme }

    component DetailLabel: Label {
        Layout.fillWidth: true
        color: theme.colors.textSecondary
        font.pixelSize: 11
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
    }

    parent: Overlay.overlay
    modal: true
    focus: true
    width: parent ? Math.max(0, Math.min(440, parent.width - 32)) : 300
    height: parent ? Math.max(0, Math.min(implicitHeight, parent.height - 32)) : implicitHeight
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

            Button {
                id: closeButton
                objectName: "settingsCloseButton"
                Layout.preferredWidth: 28
                Layout.preferredHeight: 28
                text: "✕"
                Accessible.name: qsTr("Close settings")
                onClicked: root.close()
                contentItem: Text {
                    text: "✕"
                    color: theme.colors.textSecondary
                    font.pixelSize: 16
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    radius: 6
                    color: closeButton.hovered ? theme.colors.panelHoverBg : "transparent"
                }
            }
        }

        ScrollView {
            id: settingsScroll
            objectName: "settingsScroll"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 0
            Layout.preferredHeight: registryContent.implicitHeight
            contentWidth: availableWidth
            contentHeight: registryContent.implicitHeight
            clip: true
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            ColumnLayout {
                id: registryContent
                width: settingsScroll.availableWidth
                spacing: 14

                Label {
                    text: qsTr("Registry")
                    color: theme.colors.textPrimary
                    font.bold: true
                }

                DetailLabel {
                    text: qsTr("Configured registry URL")
                }

                TextField {
                    id: registryUrlField
                    objectName: "settingsRegistryUrlField"
                    Layout.fillWidth: true
                    text: root.backend ? (root.backend.registryUrl || "") : ""
                    placeholderText: qsTr("https://…/amm-registry.json")
                    Accessible.name: qsTr("Configured registry URL")
                    color: theme.colors.textPrimary
                    background: Rectangle {
                        radius: 8
                        color: theme.colors.inputBg
                        border.color: theme.colors.border
                        border.width: 1
                    }
                }

                DetailLabel {
                    text: qsTr("Saved URL for known tokens and pools. Environment settings can override it. Leave empty to load none when no override is set.")
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

                DetailLabel {
                    objectName: "settingsRegistrySource"
                    color: theme.colors.textPrimary
                    text: {
                        switch (root.registryStatus.source) {
                        case "local": return qsTr("Active source: Local configuration")
                        case "remote": return qsTr("Active source: Remote registry")
                        case "cache": return qsTr("Active source: Cached registry")
                        default: return qsTr("Active source: None")
                        }
                    }
                }

                DetailLabel {
                    objectName: "settingsRegistrySnapshotUrl"
                    visible: root.registryStatus.snapshotUrl.length > 0
                    text: root.registryStatus.source === "cache"
                        ? qsTr("Cached from: %1").arg(root.registryStatus.snapshotUrl)
                        : qsTr("Loaded from: %1").arg(root.registryStatus.snapshotUrl)
                }

                DetailLabel {
                    objectName: "settingsRegistryEffectiveUrl"
                    visible: root.registryStatus.effectiveUrl.length > 0
                    text: qsTr("Requested registry URL: %1").arg(root.registryStatus.effectiveUrl)
                }

                DetailLabel {
                    objectName: "settingsRegistryOverride"
                    visible: root.registryStatus.override.length > 0
                    text: qsTr("Environment override: %1. Change the environment and restart to use the configured URL.")
                        .arg(root.registryStatus.override)
                }

                DetailLabel {
                    objectName: "settingsRegistryRefreshStatus"
                    visible: root.registryStatus.loading || root.registryStatus.error.length > 0
                    color: root.registryStatus.error.length > 0 ? theme.colors.warning : theme.colors.textSecondary
                    text: {
                        const retained = root.registryStatus.source !== "none"
                        if (root.registryStatus.loading)
                            return retained ? qsTr("Refreshing registry. Current data remains in use.")
                                            : qsTr("Loading registry…")
                        let error = ""
                        switch (root.registryStatus.error) {
                        case "fetch_failed": error = qsTr("Registry request failed."); break
                        case "invalid_registry": error = qsTr("Requested registry is invalid."); break
                        case "no_networks": return qsTr("Requested registry has no available networks.")
                        default: return ""
                        }
                        return retained ? qsTr("%1 Current data remains in use.").arg(error)
                                        : qsTr("%1 No registry data is loaded.").arg(error)
                    }
                }

                DetailLabel {
                    visible: networkSelector.count > 0
                    text: qsTr("Network")
                }

                ComboBox {
                    id: networkSelector
                    objectName: "settingsNetworkSelector"
                    Layout.fillWidth: true
                    visible: count > 0
                    Accessible.name: qsTr("Network")
                    textRole: "name"
                    valueRole: "id"
                    model: root.backend ? root.backend.networks : []

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

                DetailLabel {
                    objectName: "settingsNetworkUnavailable"
                    visible: networkSelector.count === 0
                    text: {
                        if (root.registryStatus.source === "local")
                            return qsTr("Network selection is unavailable with local configuration.")
                        if (root.registryStatus.loading)
                            return qsTr("Networks will appear when a registry loads.")
                        if (root.registryStatus.error === "no_networks")
                            return qsTr("Add networks to the registry or choose another registry URL.")
                        if (root.registryStatus.error.length > 0)
                            return qsTr("No networks are available because the registry could not be loaded.")
                        if (root.registryStatus.source === "none")
                            return qsTr("No registry is configured. Set a URL or an environment override to load networks.")
                        return qsTr("This registry has no available networks.")
                    }
                }
            }
        }
    }
}
