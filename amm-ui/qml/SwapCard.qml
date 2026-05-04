import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property var theme
    property var tokens: []
    property var sellToken: null
    property var buyToken: null
    property string sellAmount: ""
    property real slippageTolerancePercent: 0.5
    readonly property real feePercent: 0.30

    signal requestTokenSelect(string side)
    signal submitRequested(var snapshot)

    function setToken(side, token) {
        if (side === "sell") root.sellToken = token
        else root.buyToken = token
    }

    function resetAmounts() {
        root.sellAmount = ""
    }

    readonly property real parsedSellAmount: {
        var amt = parseFloat(sellAmount)
        return isNaN(amt) || amt < 0 ? 0 : amt
    }

    readonly property real parsedBuyAmount: {
        if (!sellToken || !buyToken || parsedSellAmount <= 0) return 0
        return parsedSellAmount * sellToken.usdPrice / buyToken.usdPrice
    }

    readonly property real minReceivedAmount: parsedBuyAmount * (1 - slippageTolerancePercent / 100)

    readonly property real priceImpactPercent: {
        if (!sellToken || parsedSellAmount <= 0) return 0
        var reserve = sellToken.reserve || 0
        if (reserve <= 0) return 0
        return parsedSellAmount / (reserve + parsedSellAmount) * 100
    }

    readonly property bool hasAmount: parsedSellAmount > 0
    readonly property bool tokensSelected: sellToken !== null && buyToken !== null
    readonly property bool insufficientBalance: hasAmount && sellToken !== null && parsedSellAmount > (sellToken.balance || 0)
    readonly property bool insufficientLiquidity: hasAmount && buyToken !== null && parsedBuyAmount > (buyToken.reserve || 0)
    readonly property bool canSubmit: tokensSelected && hasAmount && !insufficientBalance && !insufficientLiquidity

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

    readonly property string buyAmount: {
        if (!sellToken || !buyToken || sellAmount === "") return ""
        if (parsedSellAmount <= 0) return ""
        return formatAmountValue(parsedBuyAmount)
    }

    readonly property string sellUsd: {
        if (!sellToken || sellAmount === "") return ""
        if (parsedSellAmount <= 0) return ""
        var val = parsedSellAmount * sellToken.usdPrice
        return "~$" + val.toFixed(2).replace(/\B(?=(\d{3})+(?!\d))/g, ",")
    }

    readonly property string buyUsd: {
        if (!buyToken || buyAmount === "") return ""
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
            "feePercent": feePercent.toFixed(2) + "%",
            "priceImpactPercent": priceImpactPercent < 0.01 ? "<0.01%" : priceImpactPercent.toFixed(2) + "%",
            "slippageTolerance": slippageTolerancePercent.toFixed(2).replace(/0+$/, "").replace(/[.]$/, "") + "%"
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
            amount: root.sellAmount
            usdValue: root.sellUsd
            token: root.sellToken
            readOnly: false
            onInputEdited: function(v) { root.sellAmount = v }
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
            amount: root.buyAmount
            usdValue: root.buyUsd
            token: root.buyToken
            readOnly: true
            onTokenClicked: root.requestTokenSelect("buy")
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
