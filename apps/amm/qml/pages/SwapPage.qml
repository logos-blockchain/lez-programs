pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Layouts 1.15
import Logos.Wallet
import "../components/shared"
import "../components/swap"

Item {
    id: root

    // Real backend replica (logos.module("amm_ui")), wired from Main.qml.
    property var backend: null

    // Config-driven token list, loaded from AmmUiBackend::tokenList() (which
    // reads the TOKENS_CONFIG JSON file — see apps/amm/README.md). Empty
    // until the backend is ready and the call resolves.
    property var tokens: []

    // The wallet's token holdings (backend.tokenHoldings()), fed to the swap card's
    // account selectors. Refetched when the wallet opens (it needs an open wallet).
    property var holdings: []

    // Monotonic tag for refreshHoldings() requests: a callback applies only if it is still the
    // latest, so an out-of-order tokenHoldings reply can't clobber the current wallet's list.
    property int holdingsGeneration: 0

    function refreshHoldings() {
        if (!root.backend)
            return
        // onBackendChanged and onIsWalletOpenChanged can start overlapping tokenHoldings
        // requests (a wallet-open refresh racing a just-closed one, or a fast wallet switch)
        // whose replies may arrive out of order. Tag each request and drop any callback a newer
        // request has superseded, so a stale (empty, or previous-wallet) list can't overwrite
        // the current holdings.
        const generation = ++root.holdingsGeneration
        logos.watch(root.backend.tokenHoldings(),
            function(list) {
                if (generation === root.holdingsGeneration)
                    root.holdings = list
            },
            function(err) {
                if (generation === root.holdingsGeneration)
                    console.warn("tokenHoldings error:", err)
            })
    }

    onBackendChanged: {
        if (root.backend) {
            logos.watch(root.backend.tokenList(),
                function(list) { root.tokens = list },
                function(err) { console.warn("tokenList error:", err) })
            root.refreshHoldings()
        }
    }

    Connections {
        target: root.backend
        function onIsWalletOpenChanged() { root.refreshHoldings() }
    }

    QtObject {
        id: pageTheme
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
        color: pageTheme.colors.background
        Behavior on color { ColorAnimation { duration: 300 } }

        // Theme toggle
        Rectangle {
            anchors.top:    parent.top
            anchors.right:  parent.right
            anchors.margins: 16
            width: 44; height: 24; radius: 12
            color: pageTheme.colors.panelBg
            border.color: pageTheme.colors.border
            border.width: 1
            Text {
                anchors.centerIn: parent
                text: pageTheme.isDark ? "☀" : "☾"
                font.pixelSize: 13
                color: pageTheme.colors.textSecondary
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: pageTheme.isDark = !pageTheme.isDark
            }
        }

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 28

            SwapCard {
                id: swapCard
                objectName: "swapCard"
                Layout.alignment: Qt.AlignHCenter
                theme: pageTheme
                tokens: root.tokens
                holdings: root.holdings
                backend: root.backend
                width: Math.min(480, root.width - 32)

                onRequestTokenSelect: function(side) {
                    tokenModal.targetSide = side
                    // Disable the token already picked on the opposite side so the
                    // two sides can't match (a same-token pool panics amm_core).
                    var other = side === "sell" ? swapCard.buyToken : swapCard.sellToken
                    tokenModal.disabledDefinitionId = other ? other.definitionId : ""
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
                text: "Buy and sell crypto on <font color='" + pageTheme.colors.textPrimary + "'>LEZ</font>."
                textFormat: Text.RichText
                color: pageTheme.colors.textSecondary
                font.pixelSize: 15
                horizontalAlignment: Text.AlignHCenter
            }
        }

        TokenSelectorModal {
            id: tokenModal
            anchors.fill: parent
            z: 10
            theme: pageTheme
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

        Component {
            id: swapConfirmationSummary

            SwapConfirmationSummary {
                theme: pageTheme
            }
        }

        TransactionConfirmationDialog {
            id: swapConfirmationDialog
            objectName: "swapConfirmDialog"
            title: qsTr("Confirm swap")
            confirmText: qsTr("Confirm swap")
            summary: swapConfirmationSummary

            onConfirmed: function(snapshot) {
                // The dialog only shows a preview snapshot; the actual
                // on-chain swap runs here, against SwapCard's live state.
                swapCard.executeSwap()
            }
        }
    }
}
