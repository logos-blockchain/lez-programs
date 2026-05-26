import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../shared"
import "../../state"

Rectangle {
    id: root

    property var theme
    property var tokens: []
    property var sellToken: null
    property var buyToken: null
    property string sellInput: ""
    property string buyInput: ""
    property string editingSide: "sell"
    property real slippageTolerancePercent: 0.5

    DummySwapState {
        id: swapState
        feeBps: 30
    }

    signal requestTokenSelect(string side)
    signal submitRequested(var snapshot)

    function setToken(side, token) {
        if (side === "sell") root.sellToken = token
        else root.buyToken = token
    }

    function resetAmounts() {
        root.sellInput = ""
        root.buyInput = ""
        root.editingSide = "sell"
    }

    readonly property real sellReserve: sellToken ? (sellToken.reserve || 0) : 0
    readonly property real buyReserve: buyToken ? (buyToken.reserve || 0) : 0

    readonly property real parsedSellInput: {
        var amt = parseFloat(sellInput)
        return isNaN(amt) || amt < 0 ? 0 : amt
    }

    readonly property real parsedBuyInput: {
        var amt = parseFloat(buyInput)
        return isNaN(amt) || amt < 0 ? 0 : amt
    }

    readonly property real parsedSellAmount: editingSide === "sell"
        ? parsedSellInput
        : swapState.amountInFor(parsedBuyInput, sellReserve, buyReserve)

    readonly property real parsedBuyAmount: editingSide === "buy"
        ? parsedBuyInput
        : swapState.amountOutFor(parsedSellInput, sellReserve, buyReserve)

    readonly property real feeAmount: swapState.feeAmount(parsedSellAmount)
    readonly property real minReceivedAmount: swapState.minReceived(parsedBuyAmount, slippageTolerancePercent)
    readonly property real priceImpactPercent: swapState.priceImpactPercent(parsedSellAmount, parsedBuyAmount, sellReserve, buyReserve)

    readonly property string swapMode: editingSide === "buy" ? "swap-exact-output" : "swap-exact-input"
    readonly property string swapModeText: editingSide === "buy" ? qsTr("Exact output") : qsTr("Exact input")

    readonly property bool hasAmount: editingSide === "sell" ? parsedSellInput > 0 : parsedBuyInput > 0
    readonly property bool tokensSelected: sellToken !== null && buyToken !== null
    readonly property bool insufficientBalance: hasAmount && sellToken !== null && parsedSellAmount > (sellToken.balance || 0)
    readonly property bool insufficientLiquidity: hasAmount && buyToken !== null && parsedBuyAmount > (buyToken.reserve || 0)
    readonly property bool canSubmit: tokensSelected && hasAmount && parsedSellAmount > 0 && parsedBuyAmount > 0 && !insufficientBalance && !insufficientLiquidity

    readonly property string submitButtonText: {
        if (!hasAmount || !tokensSelected) return qsTr("Enter an amount")
        if (insufficientBalance) return qsTr("Insufficient balance")
        if (insufficientLiquidity) return qsTr("Insufficient liquidity")
        return qsTr("Swap")
    }

    function formatAmountValue(val) {
        if (val >= 1) return val.toFixed(2)
        if (val >= 0.0001) return val.toFixed(6)
        return val.toFixed(8)
    }

    readonly property string sellDisplay: editingSide === "sell"
        ? sellInput
        : (parsedSellAmount > 0 ? formatAmountValue(parsedSellAmount) : "")

    readonly property string buyDisplay: editingSide === "buy"
        ? buyInput
        : (parsedBuyAmount > 0 ? formatAmountValue(parsedBuyAmount) : "")

    readonly property string sellUsd: {
        if (!sellToken || parsedSellAmount <= 0) return ""
        var val = parsedSellAmount * sellToken.usdPrice
        return "~$" + val.toFixed(2).replace(/\B(?=(\d{3})+(?!\d))/g, ",")
    }

    readonly property string buyUsd: {
        if (!buyToken || parsedBuyAmount <= 0) return ""
        var val = parsedBuyAmount * buyToken.usdPrice
        return "~$" + val.toFixed(2).replace(/\B(?=(\d{3})+(?!\d))/g, ",")
    }

    function buildSnapshot() {
        return {
            "sellToken": sellToken ? sellToken.symbol : "",
            "buyToken": buyToken ? buyToken.symbol : "",
            "sellAmount": formatAmountValue(parsedSellAmount),
            "buyAmount": formatAmountValue(parsedBuyAmount),
            "minReceived": formatAmountValue(minReceivedAmount),
            "feeAmount": swapState.formatTokenAmount(feeAmount, sellToken ? sellToken.symbol : ""),
            "priceImpactPercent": swapState.formatPercent(priceImpactPercent),
            "priceImpactPercentValue": priceImpactPercent,
            "slippageTolerance": swapState.formatSlippagePercent(slippageTolerancePercent),
            "swapMode": swapMode,
            "swapModeText": swapModeText
        }
    }

    radius: 24
    color: theme.colors.cardBg
    border.color: theme.colors.border
    border.width: 1
    implicitWidth: 480
    implicitHeight: cardLayout.implicitHeight + 16

    Behavior on color { ColorAnimation { duration: 300 } }

    ColumnLayout {
        id: cardLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 8
        spacing: 0

        TokenInput {
            Layout.fillWidth: true
            theme: root.theme
            label: "Sell"
            amount: root.sellDisplay
            usdValue: root.sellUsd
            token: root.sellToken
            active: root.editingSide === "sell"
            onInputEdited: function(v) {
                root.sellInput = v
                if (root.editingSide !== "sell") root.editingSide = "sell"
            }
            onTokenClicked: root.requestTokenSelect("sell")
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 40

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.right: parent.right
                height: 1
                color: theme.colors.divider
            }

            Rectangle {
                anchors.centerIn: parent
                width: 36; height: 36; radius: 18
                color: swapHover.containsMouse ? theme.colors.panelHoverBg : theme.colors.panelBg
                border.color: theme.colors.borderStrong
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }

                Text {
                    anchors.centerIn: parent
                    text: "↓"
                    color: theme.colors.textPrimary
                    font.pixelSize: 16
                }

                MouseArea {
                    id: swapHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        var tmp = root.sellToken
                        root.sellToken = root.buyToken
                        root.buyToken = tmp
                    }
                }
            }
        }

        TokenInput {
            Layout.fillWidth: true
            theme: root.theme
            label: "Buy"
            amount: root.buyDisplay
            usdValue: root.buyUsd
            token: root.buyToken
            active: root.editingSide === "buy"
            onInputEdited: function(v) {
                root.buyInput = v
                if (root.editingSide !== "buy") root.editingSide = "buy"
            }
            onTokenClicked: root.requestTokenSelect("buy")
        }

        SwapSummary {
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            theme: root.theme
            visible: root.tokensSelected && root.hasAmount
            swapModeText: root.swapModeText
            feeText: swapState.formatTokenAmount(root.feeAmount, root.sellToken ? root.sellToken.symbol : "")
            priceImpactText: swapState.formatPercent(root.priceImpactPercent)
            priceImpactPercent: root.priceImpactPercent
            minReceivedText: swapState.formatTokenAmount(root.minReceivedAmount, root.buyToken ? root.buyToken.symbol : "")
        }

        SlippageToleranceControl {
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            tolerancePercent: root.slippageTolerancePercent
            visible: root.tokensSelected && root.hasAmount

            onToleranceChangeRequested: function(tolerancePercent) {
                root.slippageTolerancePercent = swapState.clampSlippagePercent(tolerancePercent);
            }
        }

        Rectangle {
            id: ctaBox
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.bottomMargin: 8
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.preferredHeight: 56
            radius: 20
            color: !root.canSubmit ? theme.colors.panelBg
                                   : ctaHover.containsMouse ? theme.colors.ctaHoverBg
                                                            : theme.colors.ctaBg
            Behavior on color { ColorAnimation { duration: 120 } }

            Text {
                anchors.centerIn: parent
                text: root.submitButtonText
                color: root.canSubmit ? "#ffffff" : theme.colors.textSecondary
                font.pixelSize: 17
                font.weight: Font.Medium
            }

            MouseArea {
                id: ctaHover
                anchors.fill: parent
                hoverEnabled: true
                enabled: root.canSubmit
                cursorShape: root.canSubmit ? Qt.PointingHandCursor : Qt.ArrowCursor
                onClicked: {
                    if (root.canSubmit) root.submitRequested(root.buildSnapshot())
                }
            }
        }
    }
}
