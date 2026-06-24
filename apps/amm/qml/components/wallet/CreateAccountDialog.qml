import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Public/private account creation dialog. Ported from the LEZ wallet UI.
Popup {
    id: root

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
    width: 360

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
