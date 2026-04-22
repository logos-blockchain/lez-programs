import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "components"
import "state"

Item {
    id: root

    property var tokenData: [
        { symbol: "TOK1", name: "Token 1", color: "#627eea", letter: "E", address: "0x0000000000000000000000000000000000000000",  usdPrice: 2392.70 },
        { symbol: "TOK2", name: "Token 2", color: "#2775ca", letter: "$", address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",  usdPrice: 1.00   },
        { symbol: "TOK3", name: "Token 3", color: "#26a17b", letter: "T", address: "0xdac17f958d2ee523a2206206994597c13d831ec7",  usdPrice: 1.00   },
        { symbol: "TOK4", name: "Token 4", color: "#f7931a", letter: "B", address: "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599",  usdPrice: 63500  },
        { symbol: "TOK5", name: "Token 5", color: "#627eea", letter: "E", address: "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",  usdPrice: 2392.70 },
        { symbol: "TOK6", name: "Token 6", color: "#9b59b6", letter: "L", address: "0x1337000000000000000000000000000000000cafe", usdPrice: 0.42   }
    ]

    // ── Navigation bar ────────────────────────────────────────────────────────
    // Pinned to the top; self-contained styling, unaffected by view themes.
    NavBar {
        id: navbar
        anchors.top:   parent.top
        anchors.left:  parent.left
        anchors.right: parent.right
        z: 100
    }

    // ── Content area (below the nav bar) ──────────────────────────────────────
    Item {
        anchors.top:    navbar.bottom
        anchors.left:   parent.left
        anchors.right:  parent.right
        anchors.bottom: parent.bottom

        // ── Trade view ────────────────────────────────────────────────────────
        Item {
            anchors.fill: parent
            visible: navbar.currentIndex === 0

            // Trade view theme — scoped here, invisible to NavBar and LP view.
            QtObject {
                id: theme
                property bool isDark: false
                property var colors: isDark ? dark : light

                readonly property var light: ({
                    background:      "#f7f7f5",
                    cardBg:          "#ffffff",
                    inputBg:         "#f0f0ee",
                    panelBg:         "#e8e8e4",
                    panelHoverBg:    "#ddddd8",
                    textPrimary:     "#111111",
                    textSecondary:   "#777770",
                    textPlaceholder: "#bbbbb5",
                    border:          Qt.rgba(0,0,0,0.08),
                    borderStrong:    Qt.rgba(0,0,0,0.10),
                    divider:         Qt.rgba(0,0,0,0.06),
                    ctaBg:           "#111111",
                    ctaHoverBg:      "#2a2a28",
                    selection:       "#b5c4a5",
                    noTokenCircle:   "#c8c8c4",
                    orb1:            "#7a8c6a",
                    orb2:            "#b5c4a5",
                    orb3:            "#7a8c6a",
                    orb4:            "#c8d4b8"
                })

                readonly property var dark: ({
                    background:      "#0d0d12",
                    cardBg:          "#1a1a22",
                    inputBg:         "#222230",
                    panelBg:         "#2a2a38",
                    panelHoverBg:    "#363650",
                    textPrimary:     "#ffffff",
                    textSecondary:   "#888899",
                    textPlaceholder: "#444455",
                    border:          Qt.rgba(1,1,1,0.08),
                    borderStrong:    Qt.rgba(1,1,1,0.10),
                    divider:         Qt.rgba(1,1,1,0.06),
                    ctaBg:           "#2d1530",
                    ctaHoverBg:      "#3d1f40",
                    selection:       "#4c1d4b",
                    noTokenCircle:   "#444455",
                    orb1:            "#627eea",
                    orb2:            "#9b59b6",
                    orb3:            "#fc72ff",
                    orb4:            "#26a17b"
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

                // Decorative orbs
                Rectangle { x: -180; y: -120; width: 560; height: 560; radius: 280; color: theme.colors.orb1; opacity: 0.07 }
                Rectangle { x: parent.width - 280; y: parent.height - 320; width: 480; height: 480; radius: 240; color: theme.colors.orb2; opacity: 0.09 }
                Rectangle { x: parent.width - 200; y: -80; width: 380; height: 380; radius: 190; color: theme.colors.orb3; opacity: 0.05 }
                Rectangle { x: 40; y: parent.height - 260; width: 320; height: 320; radius: 160; color: theme.colors.orb4; opacity: 0.08 }

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 28

                    SwapCard {
                        id: swapCard
                        Layout.alignment: Qt.AlignHCenter
                        theme: theme
                        tokens: root.tokenData
                        width: Math.min(480, root.width - 32)

                        onRequestTokenSelect: function(side) {
                            tokenModal.targetSide = side
                            tokenModal.open()
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
                    tokens: root.tokenData

                    property string targetSide: "sell"

                    onTokenSelected: function(tok) {
                        swapCard.setToken(targetSide, tok)
                        tokenModal.close()
                    }
                }
            }
        }

        // ── Liquidity view ────────────────────────────────────────────────────
        Item {
            anchors.fill: parent
            visible: navbar.currentIndex === 1

            DummyPoolState {
                id: poolState
            }

            Rectangle {
                anchors.fill: parent
                color: "#151515"
            }

            Flickable {
                id: scroll

                anchors.fill: parent
                clip: true
                contentHeight: content.implicitHeight + 24
                contentWidth: width

                ColumnLayout {
                    id: content
                    spacing: 10
                    width: scroll.width - 24
                    x: 12
                    y: 12

                    PoolPositionSummary {
                        poolState: poolState
                        Layout.fillWidth: true
                        Layout.preferredHeight: implicitHeight
                    }

                    AddLiquidityForm {
                        poolState: poolState
                        Layout.fillWidth: true
                        Layout.preferredHeight: implicitHeight
                    }
                }
            }
        }
    }
}
