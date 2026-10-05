import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Popup {
    id: root

    property string walletHome: ""
    property string mnemonic: ""
    property string errorText: ""
    property bool busy: false
    property bool syncing: false
    property bool syncProgressKnown: false
    property int syncCurrentBlock: 0
    property int syncTargetBlock: 0
    property int syncRemainingBlocks: 0
    property string syncError: ""

    signal createRequested(string password)
    signal copyRequested(string text)
    signal cancelSyncRequested()

    modal: true
    dim: true
    parent: Overlay.overlay
    width: Math.max(0, Math.min(420, parent ? parent.width - 32 : 420))
    x: parent ? Math.max(0, Math.round((parent.width - width) / 2)) : 0
    y: parent ? Math.max(0, Math.round((parent.height - height) / 2)) : 0
    padding: 20
    closePolicy: root.busy || root.mnemonic.length > 0
        ? Popup.NoAutoClose
        : Popup.CloseOnEscape | Popup.CloseOnPressOutside

    onOpened: {
        passwordField.text = ""
        confirmField.text = ""
        acknowledgement.checked = false
        root.errorText = ""
        root.mnemonic = ""
        passwordField.forceActiveFocus()
    }

    background: Rectangle {
        color: "#18181b"
        border.color: "#3f3f46"
        border.width: 1
        radius: 8
    }

    contentItem: ColumnLayout {
        spacing: 16

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 14
            visible: root.mnemonic.length === 0

            Label {
                Layout.fillWidth: true
                text: qsTr("Create wallet")
                color: "#f4f4f5"
                font.bold: true
                font.pixelSize: 17
            }

            Label {
                Layout.fillWidth: true
                text: root.walletHome.length > 0
                    ? qsTr("Wallet will be stored at %1").arg(root.walletHome)
                    : qsTr("Wallet will use default storage")
                color: "#a1a1aa"
                wrapMode: Text.WordWrap
            }

            TextField {
                id: passwordField
                objectName: "walletPasswordField"
                Layout.fillWidth: true
                placeholderText: qsTr("Password")
                echoMode: TextInput.Password
                enabled: !root.busy
                Keys.onReturnPressed: createButton.tryCreate()
            }

            TextField {
                id: confirmField
                objectName: "walletConfirmPasswordField"
                Layout.fillWidth: true
                placeholderText: qsTr("Confirm password")
                echoMode: TextInput.Password
                enabled: !root.busy
                Keys.onReturnPressed: createButton.tryCreate()
            }

            Label {
                Layout.fillWidth: true
                visible: root.errorText.length > 0
                text: root.errorText
                color: "#f87171"
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Button {
                    Layout.fillWidth: true
                    text: qsTr("Cancel")
                    enabled: !root.busy
                    onClicked: root.close()
                }

                Button {
                    id: createButton
                    objectName: "createWalletButton"
                    Layout.fillWidth: true
                    text: root.busy ? qsTr("Creating...") : qsTr("Create wallet")
                    enabled: !root.busy

                    function tryCreate() {
                        if (passwordField.text.length === 0) {
                            root.errorText = qsTr("Password cannot be empty.")
                        } else if (passwordField.text !== confirmField.text) {
                            root.errorText = qsTr("Passwords do not match.")
                        } else {
                            root.errorText = ""
                            root.createRequested(passwordField.text)
                        }
                    }

                    onClicked: tryCreate()
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 14
            visible: root.mnemonic.length > 0

            Label {
                Layout.fillWidth: true
                text: qsTr("Back up recovery phrase")
                color: "#f4f4f5"
                font.bold: true
                font.pixelSize: 17
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("Write these words down in order. Anyone with this phrase can control this wallet.")
                color: "#d4d4d8"
                wrapMode: Text.WordWrap
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: phraseLabel.implicitHeight + 24
                color: "#27272a"
                radius: 6

                Label {
                    id: phraseLabel
                    objectName: "walletMnemonic"
                    anchors.fill: parent
                    anchors.margins: 12
                    text: root.mnemonic
                    color: "#f4f4f5"
                    font.bold: true
                    wrapMode: Text.WordWrap
                }
            }

            Rectangle {
                Layout.fillWidth: true
                visible: root.syncing || root.syncError.length > 0
                implicitHeight: syncStatusLabel.implicitHeight + 24
                color: root.syncError.length > 0 ? "#321b1b" : "#211914"
                border.color: root.syncError.length > 0 ? "#8f3f3f" : "#6b4329"
                border.width: 1
                radius: 6

                Label {
                    id: syncStatusLabel
                    objectName: "walletSyncStatus"
                    anchors.fill: parent
                    anchors.margins: 12
                    text: root.syncError.length > 0
                        ? qsTr("Synchronization failed: %1").arg(root.syncError)
                        : root.syncProgressKnown && root.syncTargetBlock > 0
                            ? qsTr("Synchronizing wallet: %1 / %2 (%3 blocks left)")
                                .arg(root.syncCurrentBlock)
                                .arg(root.syncTargetBlock)
                                .arg(root.syncRemainingBlocks)
                            : qsTr("Synchronizing wallet…")
                    color: root.syncError.length > 0 ? "#fca5a5" : "#f2d8c7"
                    wrapMode: Text.WordWrap
                }
            }

            Button {
                objectName: "walletCancelSyncButton"
                Layout.fillWidth: true
                visible: root.syncing
                text: qsTr("Cancel synchronization")
                onClicked: root.cancelSyncRequested()
            }

            Button {
                Layout.fillWidth: true
                text: qsTr("Copy recovery phrase")
                onClicked: root.copyRequested(root.mnemonic)
            }

            CheckBox {
                id: acknowledgement
                objectName: "walletBackupAcknowledgement"
                Layout.fillWidth: true
                text: qsTr("I have safely backed up my recovery phrase")
            }

            Button {
                objectName: "walletContinueButton"
                Layout.fillWidth: true
                text: qsTr("Continue")
                enabled: acknowledgement.checked
                onClicked: root.close()
            }
        }
    }
}
