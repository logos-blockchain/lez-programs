pragma ComponentBehavior: Bound

import QtQuick

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
