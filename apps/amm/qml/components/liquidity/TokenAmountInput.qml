pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import "../shared"

AmmTokenAmountSurface {
    id: root

    property string text: ""
    property string balance: ""
    property bool balanceUpdating: false
    property string helperText: ""
    property bool showMaxButton: true
    property var tokenData: null
    property var tokens: []
    property string selectedTokenId: ""
    property bool tokenInvalid: false
    property bool tokenSelectionEnabled: true
    property var holdings: []
    property string selectedHoldingId: ""
    property bool holdingSelectionEnabled: true
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
    signal holdingSelected(string holdingId)

    amount: root.text
    supportingText: root.helperText
    supportingActionText: root.showMaxButton ? qsTr("MAX") : ""
    availableBalance: root.balance
    availableBalanceUpdating: root.balanceUpdating
    accessory: tokenActions
    accessoryWidth: width < 360 ? 132 : 180
    accessoryHeight: root.holdings.length > 1 ? 88 : 40

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

    TextEdit {
        id: clipboardProxy

        visible: false
    }

    Timer {
        id: commitTimer

        interval: 250
        repeat: false
        onTriggered: root.commitPendingEdit()
    }

    Component {
        id: tokenActions

        ColumnLayout {
            spacing: 4

            AmmTokenAccessory {
                objectName: "tokenAccessory"
                Layout.fillWidth: true
                theme: root.theme
                enabled: root.tokenSelectionEnabled
                invalid: root.tokenInvalid
                hasToken: root.tokenData !== null
                tokenColor: root.tokenColor(root.tokenData)
                tokenLetter: root.tokenLetter(root.tokenData)
                tokenText: root.tokenText(root.tokenData)
                accessibleName: qsTr("Select %1").arg(root.label)
                onClicked: tokenModal.open()
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 4
                visible: root.holdings.length > 1

                AmmSelectionComboBox {
                    id: holdingPicker

                    objectName: "holdingPicker"
                    Layout.fillWidth: true
                    theme: root.theme
                    enabled: root.holdingSelectionEnabled && root.holdings.length > 1
                    model: root.holdings
                    currentIndex: root.holdingIndex()
                    displayText: currentIndex >= 0
                                 ? root.holdingLabel(root.holdings[currentIndex])
                                 : qsTr("Select holding")
                    labelForOption: function(holding) { return root.holdingLabel(holding) }
                    tooltipText: root.selectedHoldingId
                    Accessible.name: qsTr("Wallet holding for %1").arg(root.label)
                    onActivated: function(index) {
                        root.holdingSelected(String(root.holdings[index].holdingId || ""))
                    }
                }

                AmmCopyButton {
                    objectName: "copySelectedHoldingButton"
                    Layout.preferredWidth: visible ? implicitWidth : 0
                    Layout.preferredHeight: implicitHeight
                    theme: root.theme
                    value: root.selectedHoldingId
                    onCopyRequested: function(value) { root.copyToClipboard(value) }
                }
            }
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

    function copyToClipboard(value) {
        if (!value)
            return
        clipboardProxy.text = value
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
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

    function holdingIndex() {
        for (var i = 0; i < root.holdings.length; ++i) {
            if (String(root.holdings[i].holdingId || "") === root.selectedHoldingId)
                return i
        }
        return -1
    }

    function holdingLabel(holding) {
        if (!holding)
            return qsTr("Select holding")
        return qsTr("%1 · %2")
                .arg(root.shortId(String(holding.holdingId || "")))
                .arg(String(holding.balanceRaw || "0"))
    }
}
