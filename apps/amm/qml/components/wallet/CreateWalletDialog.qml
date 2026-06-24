import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Password-only wallet creation modal. Storage/config live at the per-app
// default (backend.walletHome) — no path picking. Opened from the navbar
// "Connect" button.
Popup {
    id: root

    // Where the wallet will be stored, shown for transparency.
    property string walletHome: ""
    property string createError: ""

    signal createWallet(string password)

    modal: true
    dim: true
    padding: Theme.spacing.large
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    // Center on the full-window overlay rather than the small navbar control
    // this popup is declared inside.
    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 380

    onOpened: {
        passwordField.text = ""
        confirmField.text = ""
        root.createError = ""
        passwordField.forceActiveFocus()
    }

    background: Rectangle {
        color: Theme.palette.backgroundSecondary
        radius: Theme.spacing.radiusXlarge
        border.color: Theme.palette.backgroundElevated
    }

    contentItem: ColumnLayout {
        // Pin to the popup's padded width so long text wraps and fillWidth
        // children don't push the layout wider than the modal.
        width: root.availableWidth
        spacing: Theme.spacing.large

        LogosText {
            text: qsTr("Create your wallet")
            font.pixelSize: Theme.typography.titleText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }

        LogosText {
            text: qsTr("Secure your wallet with a password. It will be stored on this device at %1.")
                  .arg(root.walletHome || qsTr("the default location"))
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
            Layout.topMargin: -Theme.spacing.small
        }

        LogosTextField {
            id: passwordField
            Layout.fillWidth: true
            placeholderText: qsTr("Password")
            echoMode: TextInput.Password
            Keys.onReturnPressed: createButton.tryCreate()
        }
        LogosTextField {
            id: confirmField
            Layout.fillWidth: true
            placeholderText: qsTr("Confirm password")
            echoMode: TextInput.Password
            Keys.onReturnPressed: createButton.tryCreate()
        }

        LogosText {
            Layout.fillWidth: true
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.error
            wrapMode: Text.WordWrap
            visible: text.length > 0
            text: root.createError
        }

        RowLayout {
            Layout.topMargin: Theme.spacing.small
            Layout.fillWidth: true
            spacing: Theme.spacing.medium
            LogosButton {
                text: qsTr("Cancel")
                Layout.fillWidth: true
                onClicked: root.close()
            }
            LogosButton {
                id: createButton
                Layout.fillWidth: true
                text: qsTr("Create Wallet")
                function tryCreate() {
                    if (passwordField.text.length === 0) {
                        root.createError = qsTr("Password cannot be empty.")
                    } else if (passwordField.text !== confirmField.text) {
                        root.createError = qsTr("Passwords do not match.")
                    } else {
                        root.createError = ""
                        root.createWallet(passwordField.text)
                    }
                }
                onClicked: tryCreate()
            }
        }
    }
}
