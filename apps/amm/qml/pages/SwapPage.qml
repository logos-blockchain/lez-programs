import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../components/shared"
import "../components/swap"
import "../state"

Item {
    id: root

    // Real backend replica (logos.module("amm_ui")), wired from Main.qml.
    property var backend: null

    // Config-driven token list, loaded from AmmUiBackend::tokenList() (which
    // reads the TOKENS_CONFIG JSON file — see apps/amm/README.md). Empty
    // until the backend is ready and the call resolves.
    property var tokens: []

    onBackendChanged: {
        if (root.backend) {
            logos.watch(root.backend.tokenList(),
                function(list) { root.tokens = list },
                function(err) { console.warn("tokenList error:", err) })
        }
    }

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
                backend: root.backend
                width: Math.min(480, root.width - 32)

                onRequestTokenSelect: function(side) {
                    tokenModal.targetSide = side
                    tokenModal.open()
                }

                onSubmitRequested: function(snapshot) {
                    swapConfirmationDialog.openWithSnapshot(snapshot)
                }

                onSwapSucceeded: function(result) {
                    swapToast.show(qsTr("Swap submitted"),
                                   qsTr("tx %1").arg(result.txHash))
                }

                onSwapFailed: function(message) {
                    console.warn("Swap failed:", message)
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
                // The dialog only shows a preview snapshot; the actual
                // on-chain swap runs here, against SwapCard's live state.
                swapCard.executeSwap()
            }
        }
    }
}
