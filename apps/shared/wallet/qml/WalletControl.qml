pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Item {
    id: root

    property var wallet: null
    property var accountModel: null
    property var watchCall: null
    property bool compact: false
    property real viewportWidth: width
    property int selectedIndex: 0
    property bool busy: false
    property bool waitingForWallet: false
    property bool cancellationRequested: false

    readonly property bool connected: root.wallet !== null && root.wallet.isWalletOpen
    readonly property bool synchronizing: root.wallet !== null
        && root.wallet.initialSync === true
        && (root.wallet.syncStatus === "opening" || root.wallet.syncStatus === "syncing")
    readonly property bool syncFailed: root.wallet !== null
        && root.wallet.syncStatus === "error"
    readonly property bool syncProgressKnown: root.wallet !== null
        && root.wallet.syncProgressKnown === true
    readonly property string syncProgressText: {
        if (!root.synchronizing)
            return ""
        if (root.syncProgressKnown && root.wallet.syncTargetBlock > 0) {
            return qsTr("Syncing wallet · %1 / %2 · %3 blocks left")
                .arg(root.wallet.syncCurrentBlock)
                .arg(root.wallet.syncTargetBlock)
                .arg(root.wallet.syncRemainingBlocks)
        }
        return qsTr("Synchronizing wallet…")
    }
    readonly property bool compactLayout: root.compact || root.viewportWidth < 680
    readonly property string selectedAddress: root.accountAt(root.selectedIndex, "address")
    readonly property string selectedName: root.accountAt(root.selectedIndex, "name")
    readonly property string selectedBalance: root.accountAt(root.selectedIndex, "balance")
    readonly property bool selectedIsPublic: root.accountAt(root.selectedIndex, "isPublic") === true

    implicitWidth: root.synchronizing
        ? cancelSyncButton.implicitWidth
        : root.syncFailed ? syncErrorButton.implicitWidth
        : root.connected ? connectedButton.implicitWidth : connectButton.implicitWidth
    implicitHeight: 40

    Instantiator {
        id: accounts
        model: root.accountModel
        delegate: QtObject {
            required property string address
            required property string name
            required property string balance
            required property bool isPublic
        }
        onCountChanged: root.clampSelection()
    }

    function accountAt(index, field) {
        const entry = index >= 0 && index < accounts.count ? accounts.objectAt(index) : null
        return entry ? entry[field] : (field === "isPublic" ? false : "")
    }

    function clampSelection() {
        if (accounts.count === 0) {
            root.selectedIndex = 0
        } else {
            root.selectedIndex = Math.max(0, Math.min(root.selectedIndex, accounts.count - 1))
        }
    }

    function shortAddress(address) {
        return address && address.length > 13
            ? address.substring(0, 6) + "..." + address.substring(address.length - 4)
            : address || ""
    }

    function watchResult(result, success, failure) {
        if (root.watchCall) {
            root.watchCall(result, success, failure)
        } else {
            success(result)
        }
    }

    function showError(message) {
        messageDialog.message = message
        messageDialog.open()
    }

    function openWallet() {
        if (!root.wallet || root.busy)
            return
        root.busy = true
        root.waitingForWallet = true
        root.cancellationRequested = false
        try {
            root.watchResult(root.wallet.openExisting(), function(ok) {
                if (!ok) {
                    root.waitingForWallet = false
                    root.busy = false
                    root.showError(qsTr("Wallet could not be opened."))
                } else if (!root.synchronizing) {
                    root.waitingForWallet = false
                    root.busy = false
                }
            }, function(error) {
                root.waitingForWallet = false
                root.busy = false
                root.showError(qsTr("Wallet could not be opened: %1").arg(error))
            })
        } catch (error) {
            root.waitingForWallet = false
            root.busy = false
            root.showError(qsTr("Wallet could not be opened: %1").arg(error))
        }
    }

    function retryWalletSync() {
        if (!root.wallet || root.busy)
            return
        if (!root.connected) {
            root.openWallet()
            return
        }
        root.busy = true
        root.waitingForWallet = true
        root.cancellationRequested = false
        if (root.wallet.refreshAccounts)
            root.wallet.refreshAccounts()
    }

    TextEdit {
        id: clipboardProxy
        visible: false
    }

    function copyToClipboard(text) {
        if (!text)
            return
        clipboardProxy.text = text
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
    }

    Connections {
        target: root.accountModel
        ignoreUnknownSignals: true
        function onModelReset() { root.clampSelection() }
        function onRowsInserted() { root.clampSelection() }
        function onRowsRemoved() { root.clampSelection() }
    }

    Connections {
        target: root.wallet
        ignoreUnknownSignals: true

        function onSyncStatusChanged() {
            if (!root.wallet)
                return
            if (root.wallet.syncStatus === "ready") {
                root.waitingForWallet = false
                root.busy = false
                root.cancellationRequested = false
            } else if (root.wallet.syncStatus === "error"
                       || root.wallet.syncStatus === "closed") {
                const wasWaiting = root.waitingForWallet
                root.waitingForWallet = false
                root.busy = false
                if (wasWaiting || root.cancellationRequested) {
                    root.cancellationRequested = false
                    if (!(createWalletDialog.opened
                          && createWalletDialog.mnemonic.length > 0)) {
                        root.showError(root.wallet.syncError
                            ? qsTr("Wallet synchronization failed: %1").arg(root.wallet.syncError)
                            : qsTr("Wallet could not be opened."))
                    }
                }
            }
        }
    }

    onConnectedChanged: {
        if (!root.connected) {
            root.selectedIndex = 0
            walletMenu.close()
        }
    }

    onViewportWidthChanged: {
        if (walletMenu.opened)
            Qt.callLater(walletMenu.updateAnchor)
    }

    Button {
        id: syncErrorButton
        objectName: "walletSyncErrorButton"
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: root.syncFailed
        implicitHeight: 40
        implicitWidth: root.compactLayout ? 40 : 180
        text: root.compactLayout ? "!" : qsTr("Retry wallet sync")
        Accessible.name: qsTr("Retry wallet synchronization")
        ToolTip.text: root.wallet && root.wallet.syncError
            ? qsTr("Wallet synchronization failed: %1").arg(root.wallet.syncError)
            : Accessible.name
        ToolTip.visible: hovered && root.compactLayout

        background: Rectangle {
            color: syncErrorButton.pressed ? "#8f3f3f" : "#6b3030"
            border.color: "#fca5a5"
            border.width: 1
            radius: 6
        }

        contentItem: Label {
            text: syncErrorButton.text
            color: "#ffffff"
            font.bold: true
            font.pixelSize: 11
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }

        onClicked: root.retryWalletSync()
    }

    Button {
        id: cancelSyncButton
        objectName: "walletCancelSyncButton"
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: root.synchronizing
        implicitHeight: 40
        implicitWidth: root.compactLayout ? 40 : 220
        text: root.compactLayout ? "" : root.syncProgressText
        Accessible.name: qsTr("Cancel wallet synchronization")
        ToolTip.text: root.syncProgressText
        ToolTip.visible: hovered && root.compactLayout

        background: Rectangle {
            color: cancelSyncButton.pressed ? "#d95c1e" : "#f26a21"
            radius: 6

            Rectangle {
                anchors.left: parent.left
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 1
                width: root.syncProgressKnown && root.wallet.syncTargetBlock > 0
                    ? parent.width * Math.min(1,
                                              root.wallet.syncCurrentBlock
                                              / root.wallet.syncTargetBlock)
                    : 0
                height: 3
                radius: 1.5
                color: "#ffd0a8"
                visible: root.syncProgressKnown
            }
        }

        contentItem: Label {
            text: cancelSyncButton.text
            color: "#ffffff"
            font.bold: true
            font.pixelSize: 11
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }

        onClicked: {
            root.waitingForWallet = false
            root.busy = false
            root.cancellationRequested = true
            if (root.wallet)
                root.wallet.cancelSync ? root.wallet.cancelSync() : root.wallet.disconnectWallet()
        }
    }

    Button {
        id: connectButton
        objectName: "walletConnectButton"
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: !root.connected && !root.synchronizing
        enabled: root.wallet !== null && !root.busy
        implicitHeight: 40
        implicitWidth: root.compactLayout ? 40 : 108
        text: root.compactLayout ? "" : root.busy ? qsTr("Connecting...") : qsTr("Connect")
        display: root.compactLayout ? AbstractButton.IconOnly : AbstractButton.TextBesideIcon
        icon.source: Qt.resolvedUrl("icons/account.svg")
        icon.color: "#ffffff"
        icon.width: 18
        icon.height: 18
        Accessible.name: qsTr("Connect wallet")
        ToolTip.text: Accessible.name
        ToolTip.visible: hovered && root.compactLayout

        background: Rectangle {
            color: connectButton.pressed ? "#d95c1e" : "#f26a21"
            radius: 6
        }

        contentItem: RowLayout {
            spacing: 6
            Image {
                visible: root.compactLayout
                source: connectButton.icon.source
                sourceSize.width: 18
                sourceSize.height: 18
            }
            Label {
                Layout.fillWidth: true
                visible: !root.compactLayout
                text: connectButton.text
                color: "#ffffff"
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
            }
        }

        onClicked: {
            if (root.wallet && root.wallet.walletExists)
                root.openWallet()
            else
                createWalletDialog.open()
        }
    }

    Button {
        id: connectedButton
        objectName: "walletAccountButton"
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: root.connected && !root.synchronizing
        enabled: !root.busy && !root.synchronizing
        implicitHeight: 40
        implicitWidth: root.compactLayout ? 44 : Math.max(140, accountButtonLabel.implicitWidth + 58)
        Accessible.name: qsTr("Wallet account %1").arg(root.selectedAddress)

        background: Rectangle {
            color: connectedButton.pressed ? "#3f3f46" : "#27272a"
            border.width: walletMenu.opened || connectedButton.activeFocus ? 1 : 0
            border.color: "#f26a21"
            radius: 6
        }

        contentItem: RowLayout {
            spacing: 8

            Rectangle {
                Layout.preferredWidth: 8
                Layout.preferredHeight: 8
                radius: 4
                color: "#22c55e"
            }

            Label {
                id: accountButtonLabel
                Layout.fillWidth: true
                visible: !root.compactLayout
                text: root.shortAddress(root.selectedAddress) || qsTr("Connected")
                color: "#f4f4f5"
                horizontalAlignment: Text.AlignHCenter
            }

            Label {
                visible: !root.compactLayout
                text: walletMenu.opened ? "\u25b4" : "\u25be"
                color: "#a1a1aa"
            }
        }

        onClicked: {
            if (walletMenu.opened || Date.now() - walletMenu.lastClosedMs < 200)
                walletMenu.close()
            else
                walletMenu.open()
        }
    }

    Popup {
        id: walletMenu
        objectName: "walletMenu"
        property real lastClosedMs: 0
        property point anchorPosition: Qt.point(0, 0)
        readonly property var viewport: Overlay.overlay
        readonly property real spaceAbove: Math.max(0, anchorPosition.y - 20)
        readonly property real spaceBelow: viewport
            ? Math.max(0, viewport.height - anchorPosition.y - connectedButton.height - 20)
            : implicitHeight
        readonly property bool opensAbove: spaceBelow < implicitHeight && spaceAbove > spaceBelow
        readonly property real availableMenuHeight: opensAbove ? spaceAbove : spaceBelow
        parent: connectedButton
        x: viewport
            ? Math.max(12 - anchorPosition.x,
                       Math.min(connectedButton.width - width,
                                viewport.width - width - 12 - anchorPosition.x))
            : connectedButton.width - width
        y: opensAbove
            ? -height - 8
            : connectedButton.height + 8
        width: Math.min(360, Math.max(0, Math.min(root.viewportWidth,
                                                  viewport ? viewport.width : root.viewportWidth) - 24))
        height: Math.min(implicitHeight, availableMenuHeight)
        margins: 12
        padding: 12
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        function updateAnchor() {
            if (viewport)
                anchorPosition = connectedButton.mapToItem(viewport, 0, 0)
        }

        onAboutToShow: updateAnchor()

        onClosed: {
            walletMenu.lastClosedMs = Date.now()
            if (walletStack.depth > 1)
                walletStack.pop(null, StackView.Immediate)
        }

        Connections {
            target: walletMenu.opened ? walletMenu.viewport : null

            function onWidthChanged() { Qt.callLater(walletMenu.updateAnchor) }
            function onHeightChanged() { Qt.callLater(walletMenu.updateAnchor) }
        }

        background: Rectangle {
            color: "#18181b"
            border.color: "#3f3f46"
            border.width: 1
            radius: 8
        }

        contentItem: StackView {
            id: walletStack
            width: walletMenu.availableWidth
            height: walletMenu.availableHeight
            implicitWidth: walletMenu.availableWidth
            implicitHeight: currentItem ? currentItem.implicitHeight : 0
            initialItem: walletOverview
        }

        Component {
            id: walletOverview

            ColumnLayout {
                spacing: 12

                RowLayout {
                    Layout.fillWidth: true

                    Item { Layout.fillWidth: true }

                    WalletIconButton {
                        objectName: "walletAccountsButton"
                        iconSource: Qt.resolvedUrl("icons/account.svg")
                        accessibleName: qsTr("Accounts")
                        onClicked: walletStack.push(accountList)
                    }

                    WalletIconButton {
                        objectName: "walletDisconnectButton"
                        iconSource: Qt.resolvedUrl("icons/power.svg")
                        accessibleName: qsTr("Disconnect")
                        onClicked: {
                            walletMenu.close()
                            if (root.wallet)
                                root.wallet.disconnectWallet()
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: accountCard.implicitHeight + 24
                    color: "#27272a"
                    radius: 6

                    ColumnLayout {
                        id: accountCard
                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 8

                        RowLayout {
                            Layout.fillWidth: true

                            Label {
                                text: root.selectedName || qsTr("Account")
                                color: "#f4f4f5"
                                font.bold: true
                            }

                            Label {
                                text: root.selectedIsPublic ? qsTr("Public") : qsTr("Private")
                                color: "#a1a1aa"
                                font.pixelSize: 11
                            }

                            Item { Layout.fillWidth: true }

                            Label {
                                text: root.selectedBalance || "-"
                                color: "#f4f4f5"
                                font.bold: true
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 4

                            Label {
                                Layout.fillWidth: true
                                text: root.selectedAddress
                                color: "#a1a1aa"
                                font.family: "monospace"
                                font.pixelSize: 11
                                elide: Text.ElideMiddle
                            }

                            CopyButton {
                                visible: root.selectedAddress.length > 0
                                onCopyRequested: root.copyToClipboard(root.selectedAddress)
                            }
                        }
                    }
                }
            }
        }

        Component {
            id: accountList

            ColumnLayout {
                spacing: 12

                RowLayout {
                    Layout.fillWidth: true

                    WalletIconButton {
                        iconSource: Qt.resolvedUrl("icons/back.svg")
                        accessibleName: qsTr("Back")
                        onClicked: walletStack.pop()
                    }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Accounts")
                        color: "#f4f4f5"
                        font.bold: true
                    }
                }

                ListView {
                    id: accountListView
                    objectName: "walletAccountList"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumHeight: 0
                    Layout.preferredHeight: Math.min(contentHeight, 260)
                    clip: true
                    spacing: 6
                    model: root.accountModel
                    ScrollIndicator.vertical: ScrollIndicator { }

                    delegate: AccountDelegate {
                        width: ListView.view.width
                        highlighted: index === root.selectedIndex
                        onClicked: {
                            root.selectedIndex = index
                            walletStack.pop()
                        }
                        onCopyRequested: function(text) { root.copyToClipboard(text) }
                    }
                }

                Button {
                    objectName: "walletAddAccountButton"
                    Layout.fillWidth: true
                    text: qsTr("Add account")
                    enabled: !root.busy
                    onClicked: createAccountDialog.open()
                }
            }
        }
    }

    CreateWalletDialog {
        id: createWalletDialog
        objectName: "createWalletDialog"
        walletHome: root.wallet ? root.wallet.walletHome || "" : ""
        busy: root.busy
        syncing: root.synchronizing
        syncProgressKnown: root.syncProgressKnown
        syncCurrentBlock: root.wallet ? root.wallet.syncCurrentBlock || 0 : 0
        syncTargetBlock: root.wallet ? root.wallet.syncTargetBlock || 0 : 0
        syncRemainingBlocks: root.wallet ? root.wallet.syncRemainingBlocks || 0 : 0
        syncError: root.wallet ? root.wallet.syncError || "" : ""

        onCreateRequested: function(password) {
            if (!root.wallet || root.busy)
                return
            root.busy = true
            try {
                root.watchResult(root.wallet.createNewDefault(password), function(mnemonic) {
                    root.busy = false
                    if (mnemonic && mnemonic.length > 0)
                        createWalletDialog.mnemonic = mnemonic
                    else
                        createWalletDialog.errorText = qsTr("Wallet could not be created.")
                }, function(error) {
                    root.busy = false
                    createWalletDialog.errorText = qsTr("Wallet could not be created: %1").arg(error)
                })
            } catch (error) {
                root.busy = false
                createWalletDialog.errorText = qsTr("Wallet could not be created: %1").arg(error)
            }
        }

        onCopyRequested: function(text) { root.copyToClipboard(text) }
        onCancelSyncRequested: {
            root.waitingForWallet = false
            root.busy = false
            root.cancellationRequested = true
            if (root.wallet)
                root.wallet.cancelSync ? root.wallet.cancelSync() : root.wallet.disconnectWallet()
        }
    }

    CreateAccountDialog {
        id: createAccountDialog
        objectName: "createAccountDialog"
        busy: root.busy

        onCreateRequested: function(isPublic) {
            if (!root.wallet || root.busy)
                return
            root.busy = true
            try {
                const request = isPublic
                    ? root.wallet.createAccountPublic()
                    : root.wallet.createAccountPrivate()
                root.watchResult(request, function(accountId) {
                    root.busy = false
                    if (accountId && accountId.length > 0) {
                        createAccountDialog.close()
                    } else {
                        root.showError(qsTr("Account could not be created."))
                    }
                }, function(error) {
                    root.busy = false
                    root.showError(qsTr("Account could not be created: %1").arg(error))
                })
            } catch (error) {
                root.busy = false
                root.showError(qsTr("Account could not be created: %1").arg(error))
            }
        }
    }

    WalletMessageDialog {
        id: messageDialog
        objectName: "walletMessageDialog"
    }
}
