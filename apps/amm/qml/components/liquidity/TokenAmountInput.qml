pragma ComponentBehavior: Bound

import QtQuick

import Logos.Wallet

import "../shared"

AmmTokenAmountSurface {
    id: root

    property string text: ""
    property string balance: ""
    property string helperText: ""
    property bool showMaxButton: true
    property var tokenData: null
    property var tokens: []
    property string selectedTokenId: ""
    property bool tokenInvalid: false
    property bool tokenSelectionEnabled: true
    property bool editPending: false
    property string pendingValue: ""
    // Account selector (create-pool only): the wallet holdings to pick from, the
    // token's base58 definitionId to filter by, and whether to show it at all. The
    // chosen holding id is exposed as selectedHoldingId. Mirrors the swap card, where
    // the selector sits inside the input card below the token button.
    property var holdings: []
    property string holdingDefinitionId: ""
    property bool showHoldingSelector: false
    // objectName forwarded to the account selector, so UI tests can pick the
    // funding holding for this side deterministically.
    property string selectorObjectName: ""
    readonly property string selectedHoldingId: root.footerItem && root.footerItem.selectedAccountId
                                                ? String(root.footerItem.selectedAccountId) : ""

    footer: root.showHoldingSelector ? accountFooter : null
    footerHeight: root.footerItem ? root.footerItem.implicitHeight : 0
    property var disabledReasonForCode: function(code) {
        return qsTr("This token is unavailable (%1).").arg(code || "unknown")
    }
    property var detailForToken: function(token) { return "" }
    property alias popup: tokenModal
    property alias query: tokenModal.searchText
    readonly property var rows: tokenModal.rows

    signal editingChanged(string value)
    signal editingCommitted(string value)
    signal maxClicked
    signal tokenSelected(string tokenId)
    signal tokenEntered(string value)

    amount: root.text
    supportingText: root.helperText
    supportingActionText: root.showMaxButton ? qsTr("MAX") : ""
    accessory: tokenActions
    accessoryWidth: width < 360 ? 132 : 180
    accessoryHeight: root.balance.length > 0 ? 58 : 40

    onAmountEdited: function(value) {
        root.pendingValue = value
        root.editPending = true
        root.editingChanged(value)
        commitTimer.restart()
    }
    onAmountEditingFinished: function(value) {
        root.pendingValue = value
        root.commitPendingEdit()
    }
    onSupportingActionClicked: root.maxClicked()
    onTextChanged: {
        if (root.editPending && root.text !== root.pendingValue) {
            commitTimer.stop()
            root.editPending = false
        }
    }

    Timer {
        id: commitTimer

        interval: 250
        repeat: false
        onTriggered: root.commitPendingEdit()
    }

    Component {
        id: accountFooter

        // The Loader stretches this wrapper to the card width; the selector takes the
        // right half, right-aligned, matching the swap card's account selector.
        Item {
            implicitHeight: footerSelector.implicitHeight

            property alias selectedAccountId: footerSelector.selectedAccountId

            ProgramAccountSelector {
                id: footerSelector

                objectName: root.selectorObjectName
                width: Math.round(parent.width / 2)
                anchors.right: parent.right
                anchors.top: parent.top
                sourceModel: root.holdings
                accountType: "TokenHolding"
                stateField: "definitionId"
                stateValue: root.holdingDefinitionId
                selectionMode: ProgramAccountSelector.Input
                showWhenSingle: true
                textAlignment: Text.AlignRight
                backgroundColor: root.theme.colors.panelBg
                hoverColor: root.theme.colors.panelHoverBg
                textColor: root.theme.colors.textPrimary
                secondaryTextColor: root.theme.colors.textSecondary
                borderColor: root.theme.colors.borderStrong
                focusColor: root.theme.colors.ctaBg
            }
        }
    }

    Component {
        id: tokenActions

        AmmTokenAccessory {
            theme: root.theme
            enabled: root.tokenSelectionEnabled
            invalid: root.tokenInvalid
            hasToken: root.tokenData !== null
            tokenColor: root.tokenColor(root.tokenData)
            tokenLetter: root.tokenLetter(root.tokenData)
            tokenText: root.tokenText(root.tokenData)
            balance: root.balance
            accessibleName: qsTr("Select %1").arg(root.label)
            onClicked: tokenModal.open()
        }
    }

    TokenSelectorModal {
        id: tokenModal

        theme: root.theme
        tokens: root.tokens
        title: qsTr("Select a token")
        searchPlaceholder: qsTr("Search name or address")
        popularTitle: qsTr("Quick select")
        listTitle: qsTr("All tokens")
        allowCustomEntry: true
        disabledReasonForCode: root.disabledReasonForCode
        detailForToken: root.detailForToken

        onTokenSelected: function(token) {
            root.tokenSelected(String(token.definitionId || token.address || ""))
        }
        onTokenEntered: function(value) { root.tokenEntered(value) }
    }

    function acceptInput(value) {
        tokenModal.acceptInput(value)
    }

    function commitPendingEdit() {
        if (!root.editPending)
            return
        commitTimer.stop()
        root.editPending = false
        root.editingCommitted(root.pendingValue)
    }

    function tokenText(token) {
        if (!token)
            return qsTr("Select token")
        return String(token.symbol || token.name || root.shortId(root.selectedTokenId))
    }

    function tokenLetter(token) {
        var text = root.tokenText(token)
        return token ? String(token.letter || text.charAt(0).toUpperCase()) : ""
    }

    function tokenColor(token) {
        return token && token.color ? token.color : root.theme.colors.noTokenCircle
    }

    function shortId(value) {
        var text = String(value || "")
        return text.length > 14 ? text.slice(0, 7) + "..." + text.slice(-5) : text
    }
}
