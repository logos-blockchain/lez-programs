import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../shared"
import "../../state"

// Minimal, functional "manual" swap flow: the app can't yet enumerate token
// holdings with their definition ids, so the user pastes the real ids/holding
// addresses directly instead of picking from the (dummy) token list. Wires
// straight to the real backend slots (resolvePool / swapExactInput) — see
// apps/amm/src/AmmUiBackend.rep for the exact contract.
Rectangle {
    id: root

    property var theme
    property var backend: null

    property bool expanded: true

    property string defAHex: ""
    property string defBHex: ""
    property string holdingAHex: ""
    property string holdingBHex: ""
    property string amountInText: ""
    // "aToB": selling A for B. "bToA": selling B for A.
    property string direction: "aToB"
    property real slippageTolerancePercent: 0.5

    property bool poolLoading: false
    property bool poolResolved: false
    property bool poolExists: false
    property string poolReserveA: "0"
    property string poolReserveB: "0"
    property int poolFeeBps: 30
    property string poolError: ""

    property bool swapInProgress: false
    property string swapError: ""

    signal swapSucceeded(var snapshot)
    signal swapFailed(string message)

    DummySwapState {
        id: swapState
        feeBps: root.poolFeeBps
    }

    Timer {
        id: resolveDebounce
        interval: 500
        repeat: false
        onTriggered: root.doResolvePool()
    }

    function requestResolve() {
        root.poolResolved = false;
        root.poolExists = false;
        root.poolError = "";
        if (root.defAHex.length > 0 && root.defBHex.length > 0)
            resolveDebounce.restart();
        else
            resolveDebounce.stop();
    }

    onDefAHexChanged: root.requestResolve()
    onDefBHexChanged: root.requestResolve()

    function doResolvePool() {
        if (!root.backend) {
            root.poolError = qsTr("Wallet backend not ready.");
            return;
        }
        if (root.defAHex.length === 0 || root.defBHex.length === 0)
            return;

        root.poolLoading = true;
        logos.watch(root.backend.resolvePool(root.defAHex, root.defBHex),
            function (pool) {
                root.poolLoading = false;
                root.poolResolved = true;
                root.poolExists = !!(pool && pool.exists);
                root.poolReserveA = (pool && pool.reserveA) || "0";
                root.poolReserveB = (pool && pool.reserveB) || "0";
                root.poolFeeBps = (pool && pool.feeBps) || 30;
                root.poolError = "";
            },
            function (error) {
                console.warn("resolvePool error:", error);
                root.poolLoading = false;
                root.poolResolved = true;
                root.poolExists = false;
                root.poolError = qsTr("Failed to resolve pool: %1").arg(error);
            });
    }

    // JS doubles lose precision far below u128 range; this is only used to
    // format an *estimate* (expected output / min received) as a plain
    // decimal string for the backend call, never for the input amount itself
    // (which is passed through from user text verbatim).
    function formatDecimalInteger(value) {
        if (!isFinite(value) || isNaN(value) || value <= 0)
            return "0";

        var rounded = Math.floor(value);
        var s = rounded.toString();
        if (s.indexOf("e") === -1 && s.indexOf("E") === -1)
            return s;

        var match = s.match(/^(\d)(?:\.(\d+))?e\+(\d+)$/i);
        if (!match)
            return s;

        var digits = match[1] + (match[2] || "");
        var exponent = parseInt(match[3], 10);
        return digits + "0".repeat(Math.max(0, exponent - (match[2] ? match[2].length : 0)));
    }

    readonly property real reserveInNum: Number(root.direction === "aToB" ? root.poolReserveA : root.poolReserveB) || 0
    readonly property real reserveOutNum: Number(root.direction === "aToB" ? root.poolReserveB : root.poolReserveA) || 0
    readonly property real amountInNum: {
        var v = Number(root.amountInText);
        return isNaN(v) || v < 0 ? 0 : v;
    }

    readonly property real expectedOutNum: swapState.amountOutFor(root.amountInNum, root.reserveInNum, root.reserveOutNum)
    readonly property real minReceivedNum: swapState.minReceived(root.expectedOutNum, root.slippageTolerancePercent)
    readonly property real feeAmountNum: swapState.feeAmount(root.amountInNum)
    readonly property real priceImpactPercentValue: swapState.priceImpactPercent(root.amountInNum, root.expectedOutNum, root.reserveInNum, root.reserveOutNum)

    readonly property string tokenDefInHex: root.direction === "aToB" ? root.defAHex : root.defBHex
    readonly property string tokenDefOutHex: root.direction === "aToB" ? root.defBHex : root.defAHex

    readonly property bool fieldsFilled: root.defAHex.length > 0 && root.defBHex.length > 0
                                          && root.holdingAHex.length > 0 && root.holdingBHex.length > 0
                                          && root.amountInNum > 0

    readonly property bool canPreview: root.fieldsFilled && root.poolResolved && root.poolExists
    readonly property bool canSwap: root.canPreview && root.expectedOutNum > 0 && !root.swapInProgress

    readonly property string statusText: {
        if (!root.backend) return qsTr("Wallet backend not ready.");
        if (root.defAHex.length === 0 || root.defBHex.length === 0) return qsTr("Enter both token definition ids.");
        if (root.poolLoading) return qsTr("Looking up pool…");
        if (root.poolError.length > 0) return root.poolError;
        if (root.poolResolved && !root.poolExists) return qsTr("No pool / no liquidity for this pair.");
        return "";
    }

    readonly property string submitButtonText: {
        if (root.swapInProgress) return qsTr("Submitting…");
        if (!root.fieldsFilled) return qsTr("Fill in all fields");
        if (!root.poolResolved) return qsTr("Resolving pool…");
        if (!root.poolExists) return qsTr("No pool / no liquidity");
        if (root.expectedOutNum <= 0) return qsTr("Amount too small");
        return qsTr("Swap");
    }

    function resetAmount() {
        root.amountInText = "";
    }

    function doSwap() {
        if (!root.backend || !root.canSwap)
            return;

        root.swapInProgress = true;
        root.swapError = "";

        var minOutStr = root.formatDecimalInteger(root.minReceivedNum);
        // Max u64 sentinel: "ignore deadline", per AmmUiBackend.rep.
        var deadline = "18446744073709551615";

        logos.watch(root.backend.swapExactInput(root.defAHex, root.defBHex,
                                                 root.holdingAHex, root.holdingBHex,
                                                 root.amountInText, minOutStr,
                                                 root.tokenDefInHex, deadline),
            function (txHash) {
                root.swapInProgress = false;
                if (txHash && txHash.length > 0) {
                    root.swapSucceeded({
                        "txHash": txHash,
                        "amountIn": root.amountInText,
                        "sellDefHex": root.tokenDefInHex,
                        "buyDefHex": root.tokenDefOutHex,
                        "minReceived": minOutStr
                    });
                    root.resetAmount();
                    resolveDebounce.restart();
                } else {
                    root.swapError = qsTr("Swap failed (empty response from sequencer).");
                    root.swapFailed(root.swapError);
                }
            },
            function (error) {
                console.warn("swapExactInput error:", error);
                root.swapInProgress = false;
                root.swapError = qsTr("Swap error: %1").arg(error);
                root.swapFailed(root.swapError);
            });
    }

    radius: 24
    color: theme.colors.cardBg
    border.color: theme.colors.border
    border.width: 1
    implicitWidth: 480
    implicitHeight: panelLayout.implicitHeight + 32

    Behavior on color { ColorAnimation { duration: 300 } }

    ColumnLayout {
        id: panelLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 16
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Text {
                Layout.fillWidth: true
                text: qsTr("Manual swap (advanced)")
                color: theme.colors.textPrimary
                font.pixelSize: 16
                font.weight: Font.Medium
            }

            Text {
                text: root.expanded ? "▴" : "▾"
                color: theme.colors.textSecondary
                font.pixelSize: 14
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.expanded = !root.expanded
            }
        }

        Text {
            Layout.fillWidth: true
            visible: root.expanded
            text: qsTr("Paste the real token definition ids and your own holding addresses for this pair. Executes a real on-chain swap.")
            color: theme.colors.textSecondary
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: root.expanded
            spacing: 10

            // ── Direction toggle ──────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    radius: 12
                    color: root.direction === "aToB" ? theme.colors.ctaBg : theme.colors.panelBg
                    Text {
                        anchors.centerIn: parent
                        text: qsTr("Sell A → Buy B")
                        color: root.direction === "aToB" ? "#ffffff" : theme.colors.textSecondary
                        font.pixelSize: 12
                        font.weight: Font.Medium
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.direction = "aToB"
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    radius: 12
                    color: root.direction === "bToA" ? theme.colors.ctaBg : theme.colors.panelBg
                    Text {
                        anchors.centerIn: parent
                        text: qsTr("Sell B → Buy A")
                        color: root.direction === "bToA" ? "#ffffff" : theme.colors.textSecondary
                        font.pixelSize: 12
                        font.weight: Font.Medium
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.direction = "bToA"
                    }
                }
            }

            // ── Manual fields ──────────────────────────────────────────────
            ManualField {
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("Token A definition id")
                placeholder: qsTr("hex or base58")
                text: root.defAHex
                onEdited: function (v) { root.defAHex = v; }
            }

            ManualField {
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("Token B definition id")
                placeholder: qsTr("hex or base58")
                text: root.defBHex
                onEdited: function (v) { root.defBHex = v; }
            }

            ManualField {
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("Your Token A holding address")
                placeholder: qsTr("hex or base58")
                text: root.holdingAHex
                onEdited: function (v) { root.holdingAHex = v; }
            }

            ManualField {
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("Your Token B holding address")
                placeholder: qsTr("hex or base58")
                text: root.holdingBHex
                onEdited: function (v) { root.holdingBHex = v; }
            }

            ManualField {
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("Amount in (base units)")
                placeholder: qsTr("0")
                text: root.amountInText
                numericOnly: true
                onEdited: function (v) { root.amountInText = v; }
            }

            SlippageToleranceControl {
                Layout.fillWidth: true
                tolerancePercent: root.slippageTolerancePercent
                onToleranceChangeRequested: function (tolerancePercent) {
                    root.slippageTolerancePercent = swapState.clampSlippagePercent(tolerancePercent);
                }
            }

            Text {
                Layout.fillWidth: true
                visible: root.statusText.length > 0
                text: root.statusText
                color: (root.poolError.length > 0 || (root.poolResolved && !root.poolExists)) ? "#F08A76" : theme.colors.textSecondary
                font.pixelSize: 12
                wrapMode: Text.WordWrap
            }

            SwapSummary {
                Layout.fillWidth: true
                visible: root.canPreview && root.amountInNum > 0
                theme: root.theme
                feeText: swapState.formatTokenAmount(root.feeAmountNum, "")
                priceImpactText: swapState.formatPercent(root.priceImpactPercentValue)
                priceImpactPercent: root.priceImpactPercentValue
                slippageText: swapState.formatSlippagePercent(root.slippageTolerancePercent)
                minReceivedText: swapState.formatTokenAmount(root.minReceivedNum, "") + qsTr(" (base units, est.)")
            }

            Text {
                Layout.fillWidth: true
                visible: root.swapError.length > 0
                text: root.swapError
                color: "#F08A76"
                font.pixelSize: 12
                wrapMode: Text.WordWrap
            }

            Rectangle {
                id: ctaBox
                Layout.fillWidth: true
                Layout.preferredHeight: 48
                radius: 16
                color: !root.canSwap ? theme.colors.panelBg
                                     : ctaHover.containsMouse ? theme.colors.ctaHoverBg
                                                              : theme.colors.ctaBg
                Behavior on color { ColorAnimation { duration: 120 } }

                Text {
                    anchors.centerIn: parent
                    text: root.submitButtonText
                    color: root.canSwap ? "#ffffff" : theme.colors.textSecondary
                    font.pixelSize: 15
                    font.weight: Font.Medium
                }

                MouseArea {
                    id: ctaHover
                    anchors.fill: parent
                    hoverEnabled: true
                    enabled: root.canSwap
                    cursorShape: root.canSwap ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onClicked: root.doSwap()
                }
            }
        }
    }
}
