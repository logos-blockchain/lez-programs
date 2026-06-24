import QtQuick
import QtQml
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Header wallet control (Uniswap-style), with two states:
//   - not connected → a "Connect" button that opens the create-wallet modal
//   - connected     → a single button showing the active account address;
//                     clicking it opens a popup (top-right, just under the
//                     button) holding the account selector, create-account and
//                     disconnect actions.
// The selected account address is exposed via selectedAddress for the
// trade/liquidity flows to use as the "from" account.
Item {
    id: root

    // Backend replica (logos.module("amm_ui")) and its account model.
    property var backend: null
    property var accountModel: null

    readonly property bool connected: backend !== null && backend.isWalletOpen

    // Index of the active account. selectedAddress/selectedName are derived from
    // the model mirror below so they stay valid while the popup (and its list)
    // is closed.
    property int selectedIndex: 0

    // Non-visual mirror of the account model: realizes every row regardless of
    // popup visibility, so the active account is addressable by index at all
    // times (a ListView only realizes rows while it is shown).
    Instantiator {
        id: accounts
        model: root.accountModel
        delegate: QtObject {
            readonly property string address: model.address ?? ""
            readonly property string name: model.name ?? ""
            readonly property string balance: model.balance ?? ""
            readonly property bool isPublic: model.isPublic ?? false
        }
    }

    function entryAt(i) {
        return (i >= 0 && i < accounts.count) ? accounts.objectAt(i) : null
    }

    readonly property string selectedAddress: {
        const e = root.entryAt(root.selectedIndex)
        return e ? e.address : ""
    }
    readonly property string selectedName: {
        const e = root.entryAt(root.selectedIndex)
        return e ? e.name : ""
    }
    readonly property string selectedBalance: {
        const e = root.entryAt(root.selectedIndex)
        return e ? e.balance : ""
    }
    readonly property bool selectedIsPublic: {
        const e = root.entryAt(root.selectedIndex)
        return e ? e.isPublic : false
    }

    // Keep the selection within bounds as accounts are added/removed.
    function clampSelection() {
        if (accounts.count === 0) { root.selectedIndex = 0; return }
        if (root.selectedIndex < 0) root.selectedIndex = 0
        else if (root.selectedIndex >= accounts.count) root.selectedIndex = accounts.count - 1
    }
    Connections {
        target: root.accountModel
        ignoreUnknownSignals: true
        function onModelReset() { root.clampSelection() }
        function onRowsInserted() { root.clampSelection() }
        function onRowsRemoved() { root.clampSelection() }
    }

    // 0x123456…cdef style truncation for the connected button label.
    function truncated(addr) {
        if (!addr) return ""
        return addr.length > 13 ? (addr.substring(0, 6) + "…" + addr.substring(addr.length - 4)) : addr
    }

    // Copy on the QML/view side. Routing this through the backend would call
    // QGuiApplication::clipboard() in the (headless) module host process, which
    // has no clipboard — that call tears the backend down, dropping the wallet
    // connection. A hidden TextEdit copies via the GUI process that owns it.
    TextEdit { id: clipboardProxy; visible: false }
    function copyToClipboard(text) {
        if (!text) return
        clipboardProxy.text = text
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
    }

    implicitWidth: root.connected ? connectedButton.width : connectButton.width
    implicitHeight: 40

    // ── Disconnected: Connect ────────────────────────────────────────────
    LogosButton {
        id: connectButton
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        height: 40
        visible: !root.connected
        enabled: root.backend !== null
        text: qsTr("Connect")
        onClicked: {
            // Re-open an existing wallet; only show the create modal on first run.
            if (root.backend && root.backend.walletExists)
                logos.watch(root.backend.openExisting(),
                    function(ok) { if (!ok) console.warn("openExisting failed") },
                    function(error) { console.warn("openExisting error:", error) })
            else
                createWalletDialog.open()
        }
    }

    // ── Connected: address pill that toggles the wallet menu ─────────────
    Rectangle {
        id: connectedButton
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: root.connected
        implicitHeight: 40
        implicitWidth: connectedRow.implicitWidth + Theme.spacing.medium * 2
        radius: height / 2
        // Keep an opaque dark fill in both states: the navbar is white, and the
        // active "muted" fill is translucent gray, which renders light over white
        // and makes the white label unreadable. Signal "open" with an accent
        // border instead.
        color: Theme.palette.backgroundSecondary
        border.width: 1
        border.color: walletMenu.opened ? Theme.palette.overlayOrange : "transparent"

        RowLayout {
            id: connectedRow
            anchors.centerIn: parent
            spacing: Theme.spacing.small

            Rectangle {
                Layout.preferredWidth: 8
                Layout.preferredHeight: 8
                radius: 4
                color: "#39c06a"
            }
            LogosText {
                text: root.truncated(root.selectedAddress) || qsTr("Connected")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.text
            }
            LogosText {
                text: walletMenu.opened ? "▴" : "▾"
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }

        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            // CloseOnPressOutside already dismisses the popup on this same press
            // (the button is outside it), so `opened` is false by the time this
            // fires. Without the recency guard the dismissing click would just
            // reopen it. If it just closed, leave it closed.
            onClicked: {
                if (walletMenu.opened || (Date.now() - walletMenu.lastClosedMs) < 200)
                    walletMenu.close()
                else
                    walletMenu.open()
            }
        }
    }

    // ── Wallet menu popup (top-right, under the connected button) ─────────
    Popup {
        id: walletMenu
        parent: connectedButton
        y: connectedButton.height + Theme.spacing.small
        x: connectedButton.width - width   // right-align under the button
        width: 360
        padding: Theme.spacing.medium
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        // Timestamp of the last dismissal, used by the toggle button to tell a
        // genuine "open" click from the press that just closed the popup.
        property real lastClosedMs: 0
        onClosed: {
            walletMenu.lastClosedMs = Date.now()
            // Always reopen on the main (selected-account) view.
            if (viewStack.depth > 1)
                viewStack.pop(null, StackView.Immediate)
        }

        background: Rectangle {
            color: Theme.palette.backgroundTertiary
            border.width: 1
            border.color: Theme.palette.backgroundElevated
            radius: Theme.spacing.radiusLarge
        }

        // Two stacked views: the main view (active account + actions) and the
        // accounts view (full list + create). The popup height follows the
        // active view's natural height, animated so the resize isn't abrupt.
        contentItem: StackView {
            id: viewStack
            clip: true
            implicitWidth: walletMenu.availableWidth
            implicitHeight: currentItem ? currentItem.implicitHeight : 0
            initialItem: mainView

            Behavior on implicitHeight {
                NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
            }

            pushEnter: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 120 } }
            pushExit:  Transition { NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 120 } }
            popEnter:  Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 120 } }
            popExit:   Transition { NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 120 } }
        }

        // ── Main view: account icon + power icon, then the active account ──
        Component {
            id: mainView

            ColumnLayout {
                spacing: Theme.spacing.medium

                // Top-right actions: open the account list / disconnect.
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacing.small

                    Item { Layout.fillWidth: true }

                    WalletIconButton {
                        iconSource: Qt.resolvedUrl("icons/account.svg")
                        onClicked: viewStack.push(accountsView)
                    }
                    WalletIconButton {
                        iconSource: Qt.resolvedUrl("icons/settings.svg")
                        onClicked: viewStack.push(settingsView)
                    }
                    WalletIconButton {
                        iconSource: Qt.resolvedUrl("icons/power.svg")
                        onClicked: {
                            walletMenu.close()
                            if (root.backend) root.backend.disconnectWallet()
                        }
                    }
                }

                // Active account card.
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: cardColumn.implicitHeight + Theme.spacing.medium * 2
                    radius: Theme.spacing.radiusLarge
                    color: Theme.palette.backgroundMuted

                    ColumnLayout {
                        id: cardColumn
                        anchors.fill: parent
                        anchors.margins: Theme.spacing.medium
                        spacing: Theme.spacing.small

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: Theme.spacing.small

                            LogosText {
                                text: root.selectedName
                                font.pixelSize: Theme.typography.secondaryText
                                font.bold: true
                            }
                            Rectangle {
                                Layout.preferredWidth: tagLabel.implicitWidth + Theme.spacing.small * 2
                                Layout.preferredHeight: tagLabel.implicitHeight + 4
                                radius: 4
                                color: Theme.palette.backgroundSecondary
                                LogosText {
                                    id: tagLabel
                                    anchors.centerIn: parent
                                    text: root.selectedIsPublic ? qsTr("Public") : qsTr("Private")
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textSecondary
                                }
                            }
                            Item { Layout.fillWidth: true }
                            LogosText {
                                text: root.selectedBalance.length > 0 ? root.selectedBalance : "—"
                                font.bold: true
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            LogosText {
                                Layout.fillWidth: true
                                verticalAlignment: Text.AlignVCenter
                                text: root.selectedAddress
                                font.pixelSize: Theme.typography.secondaryText
                                color: Theme.palette.textMuted
                                elide: Text.ElideMiddle
                            }
                            LogosCopyButton {
                                Layout.preferredHeight: 40
                                Layout.preferredWidth: 40
                                visible: root.selectedAddress.length > 0
                                onCopyText: root.copyToClipboard(root.selectedAddress)
                                icon.color: Theme.palette.textMuted
                            }
                        }
                    }
                }
            }
        }

        // ── Accounts view: back + full list + create ──────────────────────
        Component {
            id: accountsView

            ColumnLayout {
                spacing: Theme.spacing.medium

                // Header: back to the main view + title.
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacing.small

                    WalletIconButton {
                        iconSource: Qt.resolvedUrl("icons/back.svg")
                        onClicked: viewStack.pop()
                    }
                    LogosText {
                        Layout.fillWidth: true
                        text: qsTr("Accounts")
                        font.bold: true
                        color: Theme.palette.text
                    }
                }

                // Account list: tap a row to make it the active account, then
                // return to the main view so the selection is reflected.
                ListView {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Math.min(contentHeight, 260)
                    clip: true
                    model: root.accountModel
                    spacing: Theme.spacing.small
                    ScrollIndicator.vertical: ScrollIndicator { }

                    delegate: AccountDelegate {
                        width: ListView.view.width
                        highlighted: index === root.selectedIndex
                        onClicked: {
                            root.selectedIndex = index
                            viewStack.pop()
                        }
                        onCopyRequested: (text) => root.copyToClipboard(text)
                    }
                }

                LogosButton {
                    Layout.fillWidth: true
                    height: 40
                    text: qsTr("Add")
                    // Leave the wallet menu open behind the (modal) dialog.
                    onClicked: createAccountDialog.open()
                }
            }
        }

        // ── Settings view: back + editable network (sequencer) ────────────
        Component {
            id: settingsView

            ColumnLayout {
                spacing: Theme.spacing.medium

                // Header: back to the main view + title.
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacing.small

                    WalletIconButton {
                        iconSource: Qt.resolvedUrl("icons/back.svg")
                        onClicked: viewStack.pop()
                    }
                    LogosText {
                        Layout.fillWidth: true
                        text: qsTr("Settings")
                        font.bold: true
                        color: Theme.palette.text
                    }
                }

                LogosText {
                    text: qsTr("Network (sequencer URL)")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textSecondary
                }

                LogosTextField {
                    id: seqField
                    Layout.fillWidth: true
                    placeholderText: "http://127.0.0.1:3040"
                    // Initialize from the live value without binding, so typing
                    // isn't clobbered when sequencerAddr updates after a save.
                    Component.onCompleted: text = root.backend ? root.backend.sequencerAddr : ""
                }

                LogosText {
                    id: seqStatus
                    Layout.fillWidth: true
                    visible: text.length > 0
                    wrapMode: Text.WordWrap
                    font.pixelSize: Theme.typography.secondaryText
                    property bool ok: false
                    color: ok ? Theme.palette.success : Theme.palette.error
                }

                LogosButton {
                    Layout.fillWidth: true
                    height: 40
                    text: qsTr("Save")
                    onClicked: {
                        if (!root.backend) return
                        seqStatus.text = ""
                        logos.watch(root.backend.changeSequencerAddr(seqField.text),
                            function(ok) {
                                seqStatus.ok = ok
                                seqStatus.text = ok ? qsTr("Network updated.")
                                                    : qsTr("Failed to update network.")
                            },
                            function(error) {
                                seqStatus.ok = false
                                seqStatus.text = qsTr("Error: %1").arg(error)
                            })
                    }
                }
            }
        }
    }

    // ── Dialogs ──────────────────────────────────────────────────────────
    CreateWalletDialog {
        id: createWalletDialog
        walletHome: root.backend ? root.backend.walletHome : ""
        onCreateWallet: function(password) {
            if (!root.backend) return
            // createNewDefault returns the new wallet's seed phrase (empty on
            // failure). On success we hand it to the dialog, which switches to
            // its backup page — we do NOT close here, so the user can't skip it.
            logos.watch(root.backend.createNewDefault(password),
                function(mnemonic) {
                    if (mnemonic && mnemonic.length > 0)
                        createWalletDialog.mnemonic = mnemonic
                    else
                        createWalletDialog.createError = qsTr("Failed to create wallet. Please try again.")
                },
                function(error) {
                    createWalletDialog.createError = qsTr("Error creating wallet: %1").arg(error)
                })
        }
        onCopyRequested: function(text) {
            if (root.backend) root.backend.copyToClipboard(text)
        }
    }

    CreateAccountDialog {
        id: createAccountDialog
        onCreatePublicRequested: {
            if (!root.backend) return
            logos.watch(root.backend.createAccountPublic(),
                function(_id) { /* model updates via NOTIFY after refresh */ },
                function(error) { console.warn("createAccountPublic failed:", error) })
        }
        onCreatePrivateRequested: {
            if (!root.backend) return
            logos.watch(root.backend.createAccountPrivate(),
                function(_id) { /* model updates via NOTIFY after refresh */ },
                function(error) { console.warn("createAccountPrivate failed:", error) })
        }
    }
}
