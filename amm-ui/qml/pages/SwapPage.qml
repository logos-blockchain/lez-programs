import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components/shared"
import "../components/swap"
import "../state"

Item {
    id: root

    property var tokens: [
        { symbol: "TOK1", name: "Token 1", color: "#627eea", letter: "E", address: "0x0000000000000000000000000000000000000000",  usdPrice: 2392.70, balance: 4.25,    reserve: 850     },
        { symbol: "TOK2", name: "Token 2", color: "#2775ca", letter: "$", address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",  usdPrice: 1.00,    balance: 12480,   reserve: 2400000 },
        { symbol: "TOK3", name: "Token 3", color: "#26a17b", letter: "T", address: "0xdac17f958d2ee523a2206206994597c13d831ec7",  usdPrice: 1.00,    balance: 320,     reserve: 1800000 },
        { symbol: "TOK4", name: "Token 4", color: "#f7931a", letter: "B", address: "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599",  usdPrice: 63500,   balance: 0.18,    reserve: 42      },
        { symbol: "TOK5", name: "Token 5", color: "#627eea", letter: "E", address: "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",  usdPrice: 2392.70, balance: 0,       reserve: 600     },
        { symbol: "TOK6", name: "Token 6", color: "#9b59b6", letter: "L", address: "0x1337000000000000000000000000000000000cafe", usdPrice: 0.42,    balance: 5400,    reserve: 950000  }
    ]

    QtObject {
        id: theme
        property bool isDark: true
        property var colors: isDark ? dark : light

        readonly property var light: ({
            background:      "#f4ede3",
            cardBg:          "#ffffff",
            inputBg:         "#efe7db",
            panelBg:         "#e7e1d8",
            panelHoverBg:    "#d9d0c2",
            textPrimary:     "#151515",
            textSecondary:   "#7d756e",
            textPlaceholder: "#a9a098",
            border:          Qt.rgba(0,0,0,0.08),
            borderStrong:    Qt.rgba(0,0,0,0.10),
            divider:         Qt.rgba(0,0,0,0.06),
            ctaBg:           "#f26a21",
            ctaHoverBg:      "#d95c1e",
            selection:       "#f2d8c7",
            noTokenCircle:   "#a9a098"
        })

        readonly property var dark: ({
            background:      "#151515",
            cardBg:          "#1b1b1b",
            inputBg:         "#101010",
            panelBg:         "#181818",
            panelHoverBg:    "#202020",
            textPrimary:     "#e7e1d8",
            textSecondary:   "#a9a098",
            textPlaceholder: "#8e8780",
            border:          Qt.rgba(1,1,1,0.08),
            borderStrong:    Qt.rgba(1,1,1,0.10),
            divider:         Qt.rgba(1,1,1,0.06),
            ctaBg:           "#f26a21",
            ctaHoverBg:      "#ff8a3d",
            selection:       "#211914",
            noTokenCircle:   "#343434"
        })
    }

    Rectangle {
        anchors.fill: parent
        color: theme.colors.background
        Behavior on color { ColorAnimation { duration: 300 } }

        // Theme toggle
        Rectangle {
            anchors.top:    parent.top
            anchors.right:  parent.right
            anchors.margins: 16
            width: 44; height: 24; radius: 12
            color: theme.colors.panelBg
            border.color: theme.colors.border
            border.width: 1
            Text {
                anchors.centerIn: parent
                text: theme.isDark ? "☀" : "☾"
                font.pixelSize: 13
                color: theme.colors.textSecondary
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: theme.isDark = !theme.isDark
            }
        }

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 28

            SwapCard {
                id: swapCard
                Layout.alignment: Qt.AlignHCenter
                theme: theme
                tokens: root.tokens
                width: Math.min(480, root.width - 32)

                onRequestTokenSelect: function(side) {
                    tokenModal.targetSide = side
                    tokenModal.open()
                }

                onSubmitRequested: function(snapshot) {
                    swapConfirmationDialog.openWithSnapshot(snapshot)
                }
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "Buy and sell crypto on <font color='" + theme.colors.textPrimary + "'>LEZ</font>."
                textFormat: Text.RichText
                color: theme.colors.textSecondary
                font.pixelSize: 15
                horizontalAlignment: Text.AlignHCenter
            }
        }

        TokenSelectorModal {
            id: tokenModal
            anchors.fill: parent
            z: 10
            theme: theme
            tokens: root.tokens

            property string targetSide: "sell"

            onTokenSelected: function(tok) {
                swapCard.setToken(targetSide, tok)
                tokenModal.close()
            }
        }

        SuccessToast {
            id: swapToast

            width: Math.max(0, Math.min(380, parent.width - 32))

            anchors {
                bottom: parent.bottom
                bottomMargin: 24
                horizontalCenter: parent.horizontalCenter
            }
        }

        SwapConfirmationDialog {
            id: swapConfirmationDialog
            anchors.fill: parent
            theme: theme

            onConfirmed: function(snapshot) {
                swapCard.resetAmounts()
                swapToast.show(qsTr("Swap submitted"),
                               qsTr("%1 %2 → %3 %4")
                                    .arg(snapshot.sellAmount)
                                    .arg(snapshot.sellToken)
                                    .arg(snapshot.minReceived)
                                    .arg(snapshot.buyToken))
            }
        }
    }
}
