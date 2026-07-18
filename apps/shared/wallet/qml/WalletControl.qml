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
    property int selectedIndex: -1
    property bool busy: false
    property bool openPending: false
    property bool advancedExpanded: false
    property bool availableExpanded: false
    property bool primarySelectionQueued: false
    property string postCreationWarning: ""
    property bool createdWalletAwaitingAcknowledgement: false
    property string reportedOpenErrorKey: ""

    readonly property bool connected: root.wallet !== null && root.wallet.isWalletOpen
    readonly property string syncStatus: root.wallet
                                         ? String(root.wallet.walletSyncStatus || "closed")
                                         : "closed"
    readonly property bool compactLayout: root.compact || root.viewportWidth < 680
    readonly property bool walletOpening: root.wallet !== null
        && (root.wallet.walletSyncStatus === "opening"
            || root.wallet.walletSyncStatus === "syncing")
    readonly property string selectedAddress: root.accountAt(root.selectedIndex, "address")
    readonly property string selectedDisplayAddress: root.accountAt(root.selectedIndex, "displayAddress")
    readonly property string selectedName: root.accountAt(root.selectedIndex, "name")
    readonly property string selectedBalance: root.accountAt(root.selectedIndex, "balance")
    readonly property bool selectedIsPublic: root.accountAt(root.selectedIndex, "isPublic") === true
    readonly property var walletAssets: root.wallet && root.wallet.assets ? root.wallet.assets : []
    readonly property int availableAssetCount: root.assetCount("available")
    readonly property string primaryName: root.wallet && root.wallet.primaryAccountName
        ? root.wallet.primaryAccountName : root.selectedName

    implicitWidth: root.connected ? connectedButton.implicitWidth : connectButton.implicitWidth
    implicitHeight: 40

    Instantiator {
        id: accounts
        model: root.accountModel
        delegate: QtObject {
            required property string address
            required property string displayAddress
            required property string name
            required property string balance
            required property bool isPublic
            required property bool isPrimary
            required property bool canBePrimary
            required property string kind
        }
        onCountChanged: {
            root.syncPrimarySelection()
            root.schedulePrimarySelection()
        }
        onObjectAdded: root.schedulePrimarySelection()
    }

    function accountAt(index, field) {
        const entry = index >= 0 && index < accounts.count ? accounts.objectAt(index) : null
        return entry ? entry[field] : (field === "isPublic" ? false : "")
    }

    function assetCount(sectionName) {
        let count = 0
        for (const asset of root.walletAssets) {
            if (asset.section === sectionName)
                ++count
        }
        return count
    }

    function syncPrimarySelection() {
        if (accounts.count === 0) {
            root.selectedIndex = -1
            return
        }
        const requested = root.wallet && root.wallet.primaryAccountAddress
            ? root.wallet.primaryAccountAddress : ""
        for (let index = 0; index < accounts.count; ++index) {
            const account = accounts.objectAt(index)
            if (!account)
                continue
            if ((requested.length > 0 && account.address === requested) || account.isPrimary) {
                root.selectedIndex = index
                return
            }
        }
        for (let index = 0; index < accounts.count; ++index) {
            const account = accounts.objectAt(index)
            if (!account)
                continue
            if (account.kind === "user" && account.canBePrimary) {
                root.selectedIndex = index
                return
            }
        }
        root.selectedIndex = -1
    }

    function schedulePrimarySelection() {
        if (root.primarySelectionQueued)
            return
        root.primarySelectionQueued = true
        Qt.callLater(function() {
            root.primarySelectionQueued = false
            if (root.connected)
                root.syncPrimarySelection()
            else
                root.selectedIndex = -1
        })
    }

    function shortAddress(address) {
        return address && address.length > 13
            ? address.substring(0, 6) + "…" + address.substring(address.length - 4)
            : address || ""
    }

    function watchResult(result, success, failure) {
        if (root.watchCall)
            root.watchCall(result, success, failure)
        else
            success(result)
    }

    function showError(message) {
        messageDialog.message = message
        messageDialog.open()
    }

    function walletRefreshWarning(subject) {
        if (!root.wallet || root.wallet.walletSyncStatus !== "error")
            return ""
        return qsTr("%1 was created, but could not be refreshed. Reconnect the wallet to refresh it.")
            .arg(subject)
    }

    function openFailureKey() {
        if (!root.wallet || root.wallet.walletSyncStatus !== "error")
            return ""
        return root.wallet.walletSyncError || "unknown"
    }

    function openFailureMessage() {
        const error = root.wallet ? root.wallet.walletSyncError : ""
        return error
            ? qsTr("Wallet could not be opened: %1").arg(error)
            : qsTr("Wallet could not be opened.")
    }

    function reportUnhandledOpenFailure() {
        if (!root.wallet || root.openPending || root.wallet.isWalletOpen
            || root.wallet.walletSyncStatus !== "error") {
            return
        }
        const key = root.openFailureKey()
        if (key === root.reportedOpenErrorKey)
            return
        root.reportedOpenErrorKey = key
        root.busy = false
        root.showError(root.openFailureMessage())
    }

    function openAccepted(result) {
        return result === true || result === "true" || result === 1 || result === "1"
    }

    function finishOpen() {
        root.openPending = false
        root.busy = false
    }

    function failOpen(message) {
        if (!root.openPending)
            return
        root.reportedOpenErrorKey = root.openFailureKey()
        root.finishOpen()
        root.showError(message)
    }

    function settleOpenFromWalletState() {
        if (!root.openPending || !root.wallet)
            return
        const status = root.wallet.walletSyncStatus
        if (status === undefined) {
            root.finishOpen()
            return
        }
        if (status === "ready" && root.wallet.isWalletOpen) {
            root.finishOpen()
            return
        }
        if (status === "error") {
            root.failOpen(root.openFailureMessage())
            return
        }
        if (status === "closed" && root.wallet.walletExists === false)
            root.failOpen(qsTr("Wallet could not be opened."))
    }

    function openWallet() {
        if (!root.wallet || root.busy || root.walletOpening)
            return
        root.busy = true
        root.openPending = true
        try {
            root.watchResult(root.wallet.openExisting(), function(ok) {
                if (!root.openAccepted(ok)) {
                    root.failOpen(qsTr("Wallet could not be opened."))
                    return
                }
                Qt.callLater(root.settleOpenFromWalletState)
            }, function(error) {
                root.failOpen(qsTr("Wallet could not be opened: %1").arg(error))
            })
        } catch (error) {
            root.failOpen(qsTr("Wallet could not be opened: %1").arg(error))
        }
    }

    function makePrimary(address) {
        if (!root.wallet || !address)
            return
        try {
            root.watchResult(root.wallet.setPrimaryAccount(address), function(ok) {
                if (!ok)
                    root.showError(qsTr("This account cannot be primary."))
                root.syncPrimarySelection()
            }, function(error) {
                root.showError(qsTr("Primary account could not be changed: %1").arg(error))
            })
        } catch (error) {
            root.showError(qsTr("Primary account could not be changed: %1").arg(error))
        }
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
        function onModelReset() {
            root.syncPrimarySelection()
            root.schedulePrimarySelection()
        }
        function onRowsInserted() {
            root.syncPrimarySelection()
            root.schedulePrimarySelection()
        }
        function onRowsRemoved() {
            root.syncPrimarySelection()
            root.schedulePrimarySelection()
        }
        function onDataChanged() {
            root.syncPrimarySelection()
            if (root.selectedIndex < 0)
                root.schedulePrimarySelection()
        }
    }

    Connections {
        target: root.wallet
        ignoreUnknownSignals: true
        function onPrimaryAccountAddressChanged() {
            root.syncPrimarySelection()
            root.schedulePrimarySelection()
        }
        function onWalletSyncStatusChanged() {
            if (!root.wallet || root.wallet.walletSyncStatus !== "error")
                root.reportedOpenErrorKey = ""
            Qt.callLater(root.settleOpenFromWalletState)
            Qt.callLater(root.reportUnhandledOpenFailure)
        }
        function onWalletSyncErrorChanged() {
            Qt.callLater(root.settleOpenFromWalletState)
            Qt.callLater(root.reportUnhandledOpenFailure)
        }
        function onIsWalletOpenChanged() { Qt.callLater(root.settleOpenFromWalletState) }
        function onWalletExistsChanged() { Qt.callLater(root.settleOpenFromWalletState) }
    }

    Component.onCompleted: Qt.callLater(root.reportUnhandledOpenFailure)

    onConnectedChanged: {
        if (!root.connected) {
            root.selectedIndex = -1
            walletMenu.close()
        } else {
            root.syncPrimarySelection()
            root.schedulePrimarySelection()
        }
    }
    onViewportWidthChanged: {
        if (walletMenu.opened)
            Qt.callLater(walletMenu.updateAnchor)
    }

    Button {
        id: connectButton
        objectName: "walletConnectButton"
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: !root.connected
        enabled: root.wallet !== null && !root.busy && !root.walletOpening
        implicitHeight: 40
        implicitWidth: root.compactLayout ? 40 : 108
        text: root.compactLayout ? "" : (root.busy || root.walletOpening
            ? qsTr("Connecting…") : qsTr("Connect"))
        icon.source: Qt.resolvedUrl("icons/account.svg")
        Accessible.name: qsTr("Connect AMM Wallet")
        ToolTip.text: Accessible.name
        ToolTip.visible: hovered && root.compactLayout

        background: Rectangle {
            color: connectButton.pressed ? "#d97706" : "#f59e0b"
            radius: 8
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
                color: "#18181b"
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
        visible: root.connected
        enabled: !root.busy
        implicitHeight: 40
        implicitWidth: root.compactLayout ? 44 : Math.max(176, accountButtonLabel.implicitWidth + 54)
        Accessible.name: qsTr("AMM Wallet, primary account %1").arg(root.primaryName)

        background: Rectangle {
            color: connectedButton.pressed ? "#3f3f46" : "#27272a"
            border.width: walletMenu.opened || connectedButton.activeFocus ? 1 : 0
            border.color: "#f59e0b"
            radius: 8
        }
        contentItem: RowLayout {
            spacing: 8
            Rectangle {
                Layout.preferredWidth: 8
                Layout.preferredHeight: 8
                radius: 4
                color: !root.wallet || root.wallet.networkStatus === undefined
                    || root.wallet.networkStatus === "ready"
                    ? "#22c55e"
                    : root.wallet.networkStatus === "loading" ? "#f59e0b" : "#ef4444"
            }
            Label {
                id: accountButtonLabel
                Layout.fillWidth: true
                visible: !root.compactLayout
                text: root.primaryName.length > 0
                    ? qsTr("AMM Wallet · %1").arg(root.primaryName)
                    : qsTr("AMM Wallet")
                color: "#fafafa"
                font.bold: true
                elide: Text.ElideRight
            }
            Label {
                visible: !root.compactLayout
                text: walletMenu.opened ? "▴" : "▾"
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
        palette.windowText: "#d4d4d8"
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
        y: opensAbove ? -height - 8 : connectedButton.height + 8
        width: Math.min(400, Math.max(0, Math.min(root.viewportWidth,
                                                  viewport ? viewport.width : root.viewportWidth) - 24))
        height: Math.min(implicitHeight, availableMenuHeight)
        margins: 12
        padding: 12
        focus: true
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
            radius: 10
        }

        contentItem: StackView {
            id: walletStack
            objectName: "walletStack"
            clip: true
            width: walletMenu.availableWidth
            height: walletMenu.availableHeight
            implicitWidth: walletMenu.availableWidth
            implicitHeight: currentItem ? currentItem.implicitHeight : 0
            initialItem: walletOverview
        }

        Component {
            id: walletOverview
            ScrollView {
                implicitHeight: Math.min(overviewContent.implicitHeight, 520)
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                ColumnLayout {
                    id: overviewContent
                    objectName: "walletOverviewContent"
                    width: parent.width
                    spacing: 12

                    RowLayout {
                        Layout.fillWidth: true
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1
                            Label {
                                text: qsTr("AMM Wallet")
                                color: "#fafafa"
                                font.bold: true
                                font.pixelSize: 16
                            }
                            Label {
                                text: root.wallet && root.wallet.activeNetwork
                                    ? root.wallet.activeNetwork : qsTr("Network unavailable")
                                color: "#a1a1aa"
                                font.pixelSize: 11
                            }
                        }
                        WalletIconButton {
                            objectName: "walletAccountsButton"
                            iconSource: Qt.resolvedUrl("icons/account.svg")
                            accessibleName: qsTr("Accounts")
                            onClicked: walletStack.push(accountList, StackView.Immediate)
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
                        implicitHeight: identityCard.implicitHeight + 24
                        color: "#27272a"
                        radius: 8
                        ColumnLayout {
                            id: identityCard
                            anchors.fill: parent
                            anchors.margins: 12
                            spacing: 7
                            RowLayout {
                                Layout.fillWidth: true
                                Label {
                                    Layout.fillWidth: true
                                    text: root.primaryName || qsTr("No primary account")
                                    color: "#fafafa"
                                    font.bold: true
                                    elide: Text.ElideRight
                                }
                                Label {
                                    text: qsTr("Primary")
                                    color: "#fbbf24"
                                    font.pixelSize: 11
                                    font.bold: true
                                }
                            }
                            Label {
                                objectName: "walletPrimaryAccountType"
                                visible: root.selectedIndex >= 0
                                text: root.selectedIsPublic ? qsTr("Public user account")
                                                            : qsTr("Private account")
                                color: "#a1a1aa"
                                font.pixelSize: 11
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 4
                                Label {
                                    Layout.fillWidth: true
                                    text: root.selectedDisplayAddress
                                    color: "#71717a"
                                    font.family: "monospace"
                                    font.pixelSize: 11
                                    elide: Text.ElideMiddle
                                }
                                CopyButton {
                                    visible: root.selectedDisplayAddress.length > 0
                                    onCopyRequested: root.copyToClipboard(root.selectedDisplayAddress)
                                }
                            }
                        }
                    }

                    Label {
                        text: qsTr("Assets")
                        color: "#fafafa"
                        font.bold: true
                    }
                    Label {
                        visible: root.wallet && root.wallet.assetStatus === "loading"
                        text: qsTr("Loading balances…")
                        color: "#a1a1aa"
                    }
                    Repeater {
                        objectName: "walletAssetRepeater"
                        model: root.walletAssets
                        delegate: Rectangle {
                            required property var modelData
                            objectName: "walletAssetBox"
                            Layout.fillWidth: true
                            visible: modelData.section === "assets"
                            implicitHeight: visible ? 68 : 0
                            color: "#27272a"
                            radius: 10
                            border.width: 1
                            border.color: "#3f3f46"
                            Accessible.name: qsTr("%1 token, balance %2")
                                .arg(modelData.name).arg(modelData.balance)
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 12
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Label {
                                        Layout.fillWidth: true
                                        text: modelData.name
                                        color: "#fafafa"
                                        font.bold: true
                                        elide: Text.ElideRight
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        text: root.shortAddress(modelData.displayDefinitionId
                                                                || modelData.definitionId)
                                        color: "#71717a"
                                        font.family: "monospace"
                                        font.pixelSize: 10
                                    }
                                }
                                Label {
                                    text: modelData.balance
                                    color: "#fafafa"
                                    font.family: "monospace"
                                    font.bold: true
                                }
                                CopyButton {
                                    onCopyRequested: root.copyToClipboard(
                                        modelData.displayDefinitionId || modelData.definitionId)
                                }
                            }
                        }
                    }
                    Label {
                        visible: (!root.walletAssets || root.walletAssets.length === 0)
                                 && (!root.wallet || root.wallet.assetStatus !== "loading")
                        text: root.wallet && root.wallet.assetError
                            ? qsTr("Assets unavailable: %1").arg(root.wallet.assetError)
                            : qsTr("No assets yet")
                        color: "#a1a1aa"
                        wrapMode: Text.Wrap
                    }
                    Button {
                        objectName: "walletAvailableAssetsButton"
                        Layout.fillWidth: true
                        visible: root.availableAssetCount > 0
                        text: root.availableExpanded ? qsTr("Hide available tokens")
                                                     : qsTr("Available tokens")
                        flat: true
                        onClicked: root.availableExpanded = !root.availableExpanded
                    }
                    Repeater {
                        objectName: "walletAvailableAssetRepeater"
                        model: root.walletAssets
                        delegate: Rectangle {
                            required property var modelData
                            objectName: "walletAvailableAssetBox"
                            Layout.fillWidth: true
                            visible: root.availableExpanded && modelData.section === "available"
                            implicitHeight: visible ? 64 : 0
                            color: "#202023"
                            radius: 10
                            border.width: 1
                            border.color: "#3f3f46"
                            Accessible.name: qsTr("%1 token, no balance")
                                .arg(modelData.name)
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 12
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Label {
                                        Layout.fillWidth: true
                                        text: modelData.name
                                        color: modelData.status === "ready" ? "#d4d4d8" : "#a1a1aa"
                                        elide: Text.ElideRight
                                    }
                                    Label {
                                        text: root.shortAddress(modelData.displayDefinitionId
                                                                || modelData.definitionId)
                                        color: "#71717a"
                                        font.family: "monospace"
                                        font.pixelSize: 10
                                    }
                                }
                                Label {
                                    text: modelData.status === "ready" ? "0" : qsTr("Unavailable")
                                    color: "#71717a"
                                    font.pixelSize: 11
                                }
                                CopyButton {
                                    onCopyRequested: root.copyToClipboard(
                                        modelData.displayDefinitionId || modelData.definitionId)
                                }
                            }
                        }
                    }
                }
            }
        }

        Component {
            id: accountList
            ColumnLayout {
                spacing: 10
                RowLayout {
                    Layout.fillWidth: true
                    WalletIconButton {
                        objectName: "walletAccountsBackButton"
                        iconSource: Qt.resolvedUrl("icons/back.svg")
                        accessibleName: qsTr("Back")
                        onClicked: walletStack.pop(null, StackView.Immediate)
                    }
                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Accounts")
                        color: "#fafafa"
                        font.bold: true
                    }
                }
                Label {
                    Layout.fillWidth: true
                    visible: walletMenu.availableMenuHeight >= 280
                    text: qsTr("Choose the account used as your wallet identity. Program records stay under Advanced.")
                    color: "#a1a1aa"
                    font.pixelSize: 11
                    wrapMode: Text.Wrap
                }
                ListView {
                    id: accountListView
                    objectName: "walletAccountList"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumHeight: 48
                    Layout.preferredHeight: Math.min(contentHeight, 300)
                    clip: true
                    spacing: 0
                    model: root.accountModel
                    ScrollIndicator.vertical: ScrollIndicator { }
                    delegate: Item {
                        id: accountWrapper
                        required property int index
                        required property string name
                        required property string alias
                        required property string address
                        required property string displayAddress
                        required property string balance
                        required property bool isPublic
                        required property string kind
                        required property string section
                        required property string programName
                        required property string accountType
                        required property string decodedData
                        required property string visibility
                        required property bool canBePrimary
                        required property bool isPrimary

                        readonly property bool shown: section === "accounts"
                            || (root.advancedExpanded && section === "advanced")
                        width: ListView.view.width
                        height: shown ? accountDelegate.implicitHeight + 6 : 0
                        visible: shown

                        function clicked() {
                            if (canBePrimary && !isPrimary)
                                root.makePrimary(address)
                        }

                        AccountDelegate {
                            id: accountDelegate
                            width: parent.width
                            index: accountWrapper.index
                            name: accountWrapper.name
                            alias: accountWrapper.alias
                            address: accountWrapper.address
                            displayAddress: accountWrapper.displayAddress
                            balance: accountWrapper.balance
                            isPublic: accountWrapper.isPublic
                            kind: accountWrapper.kind
                            section: accountWrapper.section
                            programName: accountWrapper.programName
                            accountType: accountWrapper.accountType
                            decodedData: accountWrapper.decodedData
                            visibility: accountWrapper.visibility
                            canBePrimary: accountWrapper.canBePrimary
                            isPrimary: accountWrapper.isPrimary
                            onMakePrimaryRequested: function(address) { root.makePrimary(address) }
                            onRenameRequested: function(address, alias) {
                                renameDialog.accountAddress = address
                                renameField.text = alias
                                renameDialog.open()
                            }
                            onCopyRequested: function(text) { root.copyToClipboard(text) }
                        }
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Button {
                        objectName: "walletAdvancedAccountsButton"
                        Layout.fillWidth: true
                        text: root.advancedExpanded ? qsTr("Hide Advanced") : qsTr("Advanced")
                        flat: true
                        onClicked: root.advancedExpanded = !root.advancedExpanded
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
    }

    Dialog {
        id: renameDialog
        objectName: "walletRenameDialog"
        property string accountAddress: ""
        parent: Overlay.overlay
        modal: true
        anchors.centerIn: parent
        width: Math.min(360, parent ? parent.width - 32 : 360)
        title: qsTr("Rename account")
        standardButtons: Dialog.Save | Dialog.Cancel
        TextField {
            id: renameField
            objectName: "walletAliasField"
            width: parent.width
            maximumLength: 40
            placeholderText: qsTr("Account name")
            Accessible.name: qsTr("Account name")
        }
        onAccepted: {
            if (!root.wallet)
                return
            try {
                root.watchResult(root.wallet.setAccountAlias(accountAddress, renameField.text),
                    function(ok) {
                        if (!ok)
                            root.showError(qsTr("Account name could not be saved."))
                    }, function(error) {
                        root.showError(qsTr("Account name could not be saved: %1").arg(error))
                    })
            } catch (error) {
                root.showError(qsTr("Account name could not be saved: %1").arg(error))
            }
        }
    }

    CreateWalletDialog {
        id: createWalletDialog
        objectName: "createWalletDialog"
        walletHome: root.wallet ? root.wallet.walletHome || "" : ""
        busy: root.busy
        onCreateRequested: function(password) {
            if (!root.wallet || root.busy)
                return
            root.postCreationWarning = ""
            root.createdWalletAwaitingAcknowledgement = false
            root.busy = true
            try {
                root.watchResult(root.wallet.createNewDefault(password), function(mnemonic) {
                    root.busy = false
                    if (mnemonic && mnemonic.length > 0) {
                        createWalletDialog.mnemonic = mnemonic
                        root.createdWalletAwaitingAcknowledgement = true
                        root.postCreationWarning = root.walletRefreshWarning(qsTr("Wallet"))
                    } else
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
        onClosed: {
            const warning = root.postCreationWarning.length > 0 ? root.postCreationWarning
                : root.createdWalletAwaitingAcknowledgement
                    ? root.walletRefreshWarning(qsTr("Wallet")) : ""
            root.postCreationWarning = ""
            root.createdWalletAwaitingAcknowledgement = false
            if (warning.length > 0)
                root.showError(warning)
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
                const request = isPublic ? root.wallet.createAccountPublic()
                                         : root.wallet.createAccountPrivate()
                root.watchResult(request, function(accountId) {
                    root.busy = false
                    if (accountId && accountId.length > 0) {
                        createAccountDialog.close()
                        Qt.callLater(function() {
                            const warning = root.walletRefreshWarning(qsTr("Account"))
                            if (warning.length > 0)
                                root.showError(warning)
                        })
                    } else
                        root.showError(qsTr("Account could not be created."))
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
