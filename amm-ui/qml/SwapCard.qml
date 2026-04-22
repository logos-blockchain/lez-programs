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

    signal requestTokenSelect(string side)

    function setToken(side, token) {
        if (side === "sell") root.sellToken = token
        else root.buyToken = token
    }

    readonly property string buyAmount: {
        if (!sellToken || !buyToken || sellAmount === "") return ""
        var amt = parseFloat(sellAmount)
        if (isNaN(amt) || amt <= 0) return ""
        var result = amt * sellToken.usdPrice / buyToken.usdPrice
        return result >= 1 ? result.toFixed(2) : result.toFixed(6)
    }

    readonly property string sellUsd: {
        if (!sellToken || sellAmount === "") return ""
        var amt = parseFloat(sellAmount)
        if (isNaN(amt)) return ""
        var val = amt * sellToken.usdPrice
        return "~$" + val.toFixed(2).replace(/\B(?=(\d{3})+(?!\d))/g, ",")
    }

    readonly property string buyUsd: {
        if (!buyToken || buyAmount === "") return ""
        var amt = parseFloat(buyAmount)
        if (isNaN(amt)) return ""
        var val = amt * buyToken.usdPrice
        return "~$" + val.toFixed(2).replace(/\B(?=(\d{3})+(?!\d))/g, ",")
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
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.bottomMargin: 8
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.preferredHeight: 56
            radius: 20
            color: ctaHover.containsMouse ? theme.colors.ctaHoverBg : theme.colors.ctaBg
            Behavior on color { ColorAnimation { duration: 120 } }

            Text {
                anchors.centerIn: parent
                text: "Swap"
                color: "#ffffff"
                font.pixelSize: 17
                font.weight: Font.Medium
            }

            MouseArea {
                id: ctaHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
            }
        }
    }
}
