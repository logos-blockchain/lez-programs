import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Wallet creation modal. Two pages, driven by whether a mnemonic exists yet:
//   1. Password entry — emits createWallet(password); the parent creates the
//      wallet and, on success, sets `mnemonic` to the returned seed phrase.
//   2. Seed-phrase backup — shows the mnemonic once and gates dismissal behind
//      an explicit acknowledgement. This is the only time the phrase is shown,
//      so the popup is not auto-dismissable while it is visible.
// Storage/config live at the per-app default (backend.walletHome) — no path
// picking. Opened from the navbar "Connect" button.
Popup {
    id: root

    // Where the wallet will be stored, shown for transparency.
    property string walletHome: ""
    property string createError: ""
    // Set by the parent to the BIP39 seed phrase once creation succeeds. A
    // non-empty value flips the dialog to the backup page.
    property string mnemonic: ""

    signal createWallet(string password)
    signal copyRequested(string text)

    modal: true
    dim: true
    padding: Theme.spacing.large
    // Once the wallet exists we must not let the user dismiss the modal (and
    // lose the only view of their seed phrase) by clicking away or pressing Esc.
    closePolicy: root.mnemonic.length > 0
                 ? Popup.NoAutoClose
                 : (Popup.CloseOnEscape | Popup.CloseOnPressOutside)
    // Center on the full-window overlay rather than the small navbar control
    // this popup is declared inside.
    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 380

    onOpened: {
        passwordField.text = ""
        confirmField.text = ""
        root.createError = ""
        root.mnemonic = ""
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
        spacing: 0

        // ── Page 1: password entry ────────────────────────────────────────
        ColumnLayout {
            id: passwordPage
            visible: root.mnemonic.length === 0
            Layout.fillWidth: true
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

        // ── Page 2: seed-phrase backup ────────────────────────────────────
        ColumnLayout {
            id: backupPage
            visible: root.mnemonic.length > 0
            Layout.fillWidth: true
            spacing: Theme.spacing.large

            LogosText {
                text: qsTr("Back up your recovery phrase")
                font.pixelSize: Theme.typography.titleText
                font.weight: Theme.typography.weightBold
                color: Theme.palette.text
            }

            LogosText {
                text: qsTr("Write these words down in order and store them somewhere safe. Anyone with this phrase can control your wallet, and it will not be shown again — it is the only way to recover access.")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
                Layout.topMargin: -Theme.spacing.small
            }

            Rectangle {
                Layout.fillWidth: true
                radius: Theme.spacing.radiusLarge
                color: Theme.palette.backgroundElevated
                implicitHeight: phraseText.implicitHeight + 2 * Theme.spacing.medium

                LogosText {
                    id: phraseText
                    anchors.fill: parent
                    anchors.margins: Theme.spacing.medium
                    text: root.mnemonic
                    wrapMode: Text.WordWrap
                    lineHeight: 1.4
                    font.pixelSize: Theme.typography.primaryText
                    font.weight: Theme.typography.weightBold
                    color: Theme.palette.text
                }
            }

            LogosButton {
                Layout.fillWidth: true
                text: qsTr("Copy to clipboard")
                onClicked: root.copyRequested(root.mnemonic)
            }

            LogosCheckbox {
                id: ackCheck
                Layout.fillWidth: true
                text: qsTr("I have safely backed up my recovery phrase")
            }

            LogosButton {
                Layout.fillWidth: true
                enabled: ackCheck.checked
                text: qsTr("Continue")
                onClicked: root.close()
            }
        }
    }
}
