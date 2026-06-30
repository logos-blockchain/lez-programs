import QtQuick 2.15
import QtQuick.Layouts 1.15
import "../components/shared"
import "../components/swap"

Item {
    id: root

    property var tokens: []
    property var poolConfig: ({})
    property var backend: null
    property bool unsupportedChain: false
    property string selectedWalletAccount: ""
    readonly property string poolAccount: poolConfig.account || ""
    readonly property string poolAccountShort: poolAccount.length > 14
                                               ? poolAccount.substring(0, 8) + "..." + poolAccount.slice(-6)
                                               : poolAccount

    onTokensChanged: Qt.callLater(selectDefaultTokens)

    Component.onCompleted: Qt.callLater(selectDefaultTokens)

    function selectDefaultTokens() {
        if (root.tokens.length < 2) {
            swapCard.setToken("sell", null);
            swapCard.setToken("buy", null);
            swapCard.resetAmounts();
            return;
        }
        if (!swapCard.sellToken || swapCard.sellToken.address !== root.tokens[0].address)
            swapCard.setToken("sell", root.tokens[0]);
        if (!swapCard.buyToken || swapCard.buyToken.address !== root.tokens[1].address)
            swapCard.setToken("buy", root.tokens[1]);
    }

    function parseTransactionResult(resultJson) {
        try {
            return JSON.parse(resultJson);
        } catch (err) {
            return { "success": false, "tx_hash": "", "error": String(err) };
        }
    }

    function withSelectedWalletAccount(snapshot) {
        snapshot.selectedWalletAccount = root.selectedWalletAccount
        return snapshot
    }

    function submissionToken() {
        if (!root.backend)
            return ""
        return [
            root.backend.isWalletOpen ? "open" : "closed",
            root.backend.sequencerAddr || "",
            root.backend.deploymentNetworkMatched ? "matched" : "unmatched",
            root.selectedWalletAccount || ""
        ].join("|")
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
                visible: !root.unsupportedChain
                Layout.alignment: Qt.AlignHCenter
                theme: theme
                tokens: root.tokens
                feeBps: Number(root.poolConfig.feeBps) || 0
                Layout.preferredWidth: Math.min(480, root.width - 32)

                onRequestTokenSelect: function(side) {
                    tokenModal.targetSide = side
                    tokenModal.open()
                }

                onSubmitRequested: function(snapshot) {
                    swapConfirmationDialog.openWithSnapshot(snapshot)
                }
            }

            Text {
                visible: !root.unsupportedChain
                Layout.alignment: Qt.AlignHCenter
                text: "Pool <font color='" + theme.colors.textPrimary + "'>" +
                      root.poolAccountShort +
                      "</font>"
                textFormat: Text.RichText
                color: theme.colors.textSecondary
                font.pixelSize: 15
                horizontalAlignment: Text.AlignHCenter
            }

            Text {
                visible: root.unsupportedChain
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("Unsupported chain")
                color: theme.colors.textPrimary
                font.pixelSize: 18
                font.bold: true
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
                if (!root.backend) {
                    swapToast.show(qsTr("Swap failed"), qsTr("Backend is not ready"), "", "error")
                    return
                }

                const expectedSubmissionToken = root.submissionToken()
                logos.watch(root.backend.submitSwap(root.withSelectedWalletAccount(snapshot)),
                    function(resultJson) {
                        if (root.submissionToken() !== expectedSubmissionToken)
                            return
                        const result = root.parseTransactionResult(resultJson)
                        if (!result.success) {
                            swapToast.show(qsTr("Swap failed"),
                                           result.error || qsTr("Transaction rejected"),
                                           "",
                                           "error")
                            return
                        }

                        swapCard.resetAmounts()
                        swapToast.show(qsTr("Swap submitted"),
                                       qsTr("%1 %2 → %3 %4")
                                            .arg(snapshot.sellAmount)
                                            .arg(snapshot.sellToken)
                                            .arg(snapshot.minReceived)
                                            .arg(snapshot.buyToken),
                                       result.tx_hash || "")
                    },
                    function(error) {
                        if (root.submissionToken() !== expectedSubmissionToken)
                            return
                        swapToast.show(qsTr("Swap failed"), String(error), "", "error")
                    })
            }
        }
    }
}
