import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

import Logos.Theme
import Logos.Controls

// Public/private account creation dialog. Ported from the LEZ wallet UI.
Popup {
    id: root

    readonly property real viewportMargin: Theme.spacing.large

    signal createPublicRequested()
    signal createPrivateRequested()

    modal: true
    dim: true
    padding: Theme.spacing.large
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    // Center on the full-window overlay rather than the small navbar control
    // this popup is declared inside.
    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(360, Math.max(0, parent ? parent.width - root.viewportMargin * 2 : 360))

    background: Rectangle {
        color: Theme.palette.backgroundSecondary
        radius: Theme.spacing.radiusXlarge
        border.color: Theme.palette.backgroundElevated
    }

    contentItem: ColumnLayout {
        id: contentLayout
        // Pin to the popup's padded width so children stay within the modal.
        width: root.availableWidth
        spacing: Theme.spacing.large

        LogosText {
            text: qsTr("Create account")
            font.pixelSize: Theme.typography.titleText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }

        LogosText {
            text: qsTr("Choose account type.")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
            Layout.topMargin: -Theme.spacing.small
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing.medium

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                LogosText {
                    text: qsTr("Private")
                    font.pixelSize: Theme.typography.primaryText
                    color: Theme.palette.text
                }
                LogosText {
                    text: qsTr("Private balance and activity.")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textSecondary
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }

            LogosSwitch {
                id: privateSwitch
                checked: false
                Accessible.name: qsTr("Create private account")
                Accessible.description: qsTr("Private balance and activity.")
            }
        }

        RowLayout {
            Layout.topMargin: Theme.spacing.medium
            spacing: Theme.spacing.medium
            Layout.fillWidth: true
            LogosButton {
                text: qsTr("Cancel")
                Layout.fillWidth: true
                onClicked: root.close()
            }
            LogosButton {
                text: qsTr("Create")
                Layout.fillWidth: true
                onClicked: {
                    if (privateSwitch.checked)
                        root.createPrivateRequested()
                    else
                        root.createPublicRequested()
                    root.close()
                }
            }
        }
    }
}
