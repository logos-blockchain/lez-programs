import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../shared"
import "../../state"

// The real swap UI: two token inputs (sell/buy), a token picker (backed by
// AmmUiBackend::tokenList()/TOKENS_CONFIG via SwapPage), and a submit flow
// wired straight to the backend's resolvePool()/swapExactInput() slots — see
// apps/amm/src/AmmUiBackend.rep for the exact contract.
Rectangle {
    id: root

    property var theme
    property var tokens: []
    // Real backend replica (logos.module("amm_ui")), wired from SwapPage.
    property var backend: null

    property var sellToken: null
    property var buyToken: null
    property string sellInput: ""
    property string buyInput: ""
    property string editingSide: "sell"
    property real slippageTolerancePercent: 0.5

    // ── Pool resolution (backend.resolvePool) ───────────────────────────────
    // A pool's PoolDefinition stores def_a/def_b in the pool CREATOR's order
    // (from NewDefinition), not sorted, and not necessarily in (sellToken,
    // buyToken) order. reserveA/reserveB mirror that same canonical order, so
    // they must be mapped to sell/buy via poolDefAHex below — never assumed.
    property bool poolLoading: false
    property bool poolResolved: false
    property bool poolExists: false
    property string poolReserveA: "0"
    property string poolReserveB: "0"
    property string poolDefAHex: ""
    property int poolFeeBps: 30
    property string poolError: ""

    // ── Swap submission (backend.swapExactInput) ────────────────────────────
    property bool swapInProgress: false
    property string swapError: ""

    signal requestTokenSelect(string side)
    signal submitRequested(var snapshot)
    // Emitted after a real swapExactInput call completes.
    signal swapSucceeded(var result)
    signal swapFailed(string message)

    DummySwapState {
        id: swapState
        feeBps: root.poolFeeBps
    }

    function setToken(side, token) {
        if (side === "sell") root.sellToken = token
        else root.buyToken = token
    }

    function resetAmounts() {
        root.sellInput = ""
        root.buyInput = ""
        root.editingSide = "sell"
    }

    // ── Pool resolution ──────────────────────────────────────────────────────
    Timer {
        id: resolveDebounce
        interval: 500
        repeat: false
        onTriggered: root.doResolvePool()
    }

    function requestResolve() {
        root.poolResolved = false
        root.poolExists = false
        root.poolError = ""
        if (root.sellToken && root.buyToken)
            resolveDebounce.restart()
        else
            resolveDebounce.stop()
    }

    onSellTokenChanged: root.requestResolve()
    onBuyTokenChanged: root.requestResolve()

    function doResolvePool() {
        if (!root.backend || !root.sellToken || !root.buyToken)
            return

        root.poolLoading = true
        logos.watch(root.backend.resolvePool(root.sellToken.definitionId, root.buyToken.definitionId),
            function (pool) {
                root.poolLoading = false
                root.poolResolved = true
                root.poolExists = !!(pool && pool.exists)
                root.poolReserveA = (pool && pool.reserveA) || "0"
                root.poolReserveB = (pool && pool.reserveB) || "0"
                root.poolDefAHex = (pool && pool.defAHex) || ""
                // feeBps === 0 is a legitimate zero-fee pool; only fall back
                // to the 30bps default when the backend didn't send a value.
                root.poolFeeBps = (pool && pool.feeBps !== undefined) ? pool.feeBps : 30
                root.poolError = (pool && pool.error && pool.error !== "no_pool") ? pool.error : ""
            },
            function (error) {
                console.warn("resolvePool error:", error)
                root.poolLoading = false
                root.poolResolved = true
                root.poolExists = false
                root.poolError = qsTr("Failed to resolve pool: %1").arg(error)
            })
    }

    // JS doubles lose precision far below u128 range; these are only used to
    // drive the *estimate* (expected output / min received / price impact),
    // never the actual swap amount — the sell amount sent to the backend is
    // the user's raw input text, passed through verbatim.
    // The pool's reserveA/reserveB follow the pool's canonical def_a/def_b
    // order (see poolDefAHex above), which may or may not match sell/buy —
    // map them explicitly rather than assuming reserveA == sell.
    readonly property bool sellIsPoolA: !!root.sellToken && root.sellToken.definitionId === root.poolDefAHex
    readonly property real sellReserveNum: Number(sellIsPoolA ? root.poolReserveA : root.poolReserveB) || 0
    readonly property real buyReserveNum: Number(sellIsPoolA ? root.poolReserveB : root.poolReserveA) || 0

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
        : swapState.amountInFor(parsedBuyInput, sellReserveNum, buyReserveNum)

    readonly property real parsedBuyAmount: editingSide === "buy"
        ? parsedBuyInput
        : swapState.amountOutFor(parsedSellInput, sellReserveNum, buyReserveNum)

    readonly property real feeAmount: swapState.feeAmount(parsedSellAmount)
    readonly property real minReceivedAmount: swapState.minReceived(parsedBuyAmount, slippageTolerancePercent)
    readonly property real priceImpactPercent: swapState.priceImpactPercent(parsedSellAmount, parsedBuyAmount, sellReserveNum, buyReserveNum)

    readonly property string swapModeText: editingSide === "buy" ? qsTr("Exact output (preview only)") : qsTr("Exact input")

    readonly property bool hasAmount: editingSide === "sell" ? parsedSellInput > 0 : parsedBuyInput > 0
    readonly property bool tokensSelected: sellToken !== null && buyToken !== null
    readonly property bool insufficientLiquidity: hasAmount && root.poolExists && parsedBuyAmount > buyReserveNum
    // The backend only exposes swapExactInput, so only the "I know exactly
    // how much I'm selling" direction can actually be submitted. Editing the
    // buy field still previews an estimate (via amountInFor above) but can't
    // be submitted — see doc comment on AmmUiBackend::swapExactInput.
    readonly property bool canSubmit: tokensSelected && editingSide === "sell" && hasAmount
                                       && parsedSellAmount > 0 && parsedBuyAmount > 0
                                       && root.poolResolved && root.poolExists
                                       && !insufficientLiquidity && !root.swapInProgress

    readonly property string submitButtonText: {
        if (!tokensSelected) return qsTr("Select tokens")
        if (root.swapInProgress) return qsTr("Submitting…")
        if (!hasAmount) return qsTr("Enter an amount")
        if (editingSide === "buy") return qsTr("Enter a sell amount to swap")
        if (root.poolLoading || !root.poolResolved) return qsTr("Resolving pool…")
        if (!root.poolExists) return qsTr("No pool / no liquidity")
        if (insufficientLiquidity) return qsTr("Insufficient liquidity")
        if (parsedBuyAmount <= 0) return qsTr("Amount too small")
        return qsTr("Swap")
    }

    readonly property string poolStatusText: {
        if (!root.backend) return qsTr("Wallet backend not ready.")
        if (!tokensSelected) return ""
        if (root.poolLoading) return qsTr("Looking up pool…")
        if (root.poolError.length > 0) return root.poolError
        if (root.poolResolved && !root.poolExists) return qsTr("No pool / no liquidity for this pair.")
        return ""
    }

    function formatAmountValue(val) {
        if (val >= 1) return val.toFixed(2)
        if (val >= 0.0001) return val.toFixed(6)
        return val.toFixed(8)
    }

    // Base units are integers; render the (estimate-only) computed side as a
    // plain integer string rather than a fractional/scientific one.
    function formatBaseUnits(value) {
        if (!isFinite(value) || isNaN(value) || value <= 0)
            return "0"

        var rounded = Math.floor(value)
        var s = rounded.toString()
        if (s.indexOf("e") === -1 && s.indexOf("E") === -1)
            return s

        var match = s.match(/^(\d)(?:\.(\d+))?e\+(\d+)$/i)
        if (!match)
            return s

        var digits = match[1] + (match[2] || "")
        var exponent = parseInt(match[3], 10)
        return digits + "0".repeat(Math.max(0, exponent - (match[2] ? match[2].length : 0)))
    }

    readonly property string sellDisplay: editingSide === "sell"
        ? sellInput
        : (parsedSellAmount > 0 ? formatBaseUnits(parsedSellAmount) : "")

    readonly property string buyDisplay: editingSide === "buy"
        ? buyInput
        : (parsedBuyAmount > 0 ? formatBaseUnits(parsedBuyAmount) : "")

    function buildSnapshot() {
        return {
            "sellToken": sellToken ? sellToken.symbol : "",
            "buyToken": buyToken ? buyToken.symbol : "",
            "sellAmount": formatBaseUnits(parsedSellAmount),
            "buyAmount": formatBaseUnits(parsedBuyAmount),
            "minReceived": formatBaseUnits(minReceivedAmount),
            "feeAmount": swapState.formatTokenAmount(feeAmount, sellToken ? sellToken.symbol : ""),
            "priceImpactPercent": swapState.formatPercent(priceImpactPercent),
            "priceImpactPercentValue": priceImpactPercent,
            "slippageTolerance": swapState.formatSlippagePercent(slippageTolerancePercent),
            "swapMode": "swap-exact-input",
            "swapModeText": swapModeText
        }
    }

    // Called by SwapPage once the user confirms in SwapConfirmationDialog.
    // Submits the real on-chain swap for the amounts/tokens selected when the
    // CTA was pressed (canSubmit already guarantees editingSide === "sell").
    function executeSwap() {
        if (!root.backend || !root.canSubmit)
            return

        root.swapInProgress = true
        root.swapError = ""

        // Compute the submitted slippage floor with exact integer (BigInt) math
        // rather than the double-based preview: base-unit values for 18-decimal
        // tokens exceed 2^53, where doubles would understate min_out and weaken
        // price protection. Sell/buy reserves follow the pool's canonical order.
        var minOutStr = swapState.minOutBaseUnits(
                root.sellInput,
                root.sellIsPoolA ? root.poolReserveA : root.poolReserveB,
                root.sellIsPoolA ? root.poolReserveB : root.poolReserveA,
                root.slippageTolerancePercent)
        // Max u64 sentinel: "ignore deadline", per AmmUiBackend.rep.
        var deadline = "18446744073709551615"

        logos.watch(root.backend.swapExactInput(
                root.sellToken.definitionId, root.buyToken.definitionId,
                root.sellToken.holding, root.buyToken.holding,
                root.sellInput, minOutStr, deadline),
            function (txHash) {
                root.swapInProgress = false
                if (txHash && txHash.length > 0) {
                    root.swapSucceeded({
                        "txHash": txHash,
                        "sellToken": root.sellToken.symbol,
                        "buyToken": root.buyToken.symbol,
                        "sellAmount": root.sellInput,
                        "minReceived": minOutStr
                    })
                    root.resetAmounts()
                    resolveDebounce.restart()
                } else {
                    root.swapError = qsTr("Swap failed (empty response from sequencer).")
                    root.swapFailed(root.swapError)
                }
            },
            function (error) {
                console.warn("swapExactInput error:", error)
                root.swapInProgress = false
                root.swapError = qsTr("Swap error: %1").arg(error)
                root.swapFailed(root.swapError)
            })
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
            token: root.sellToken
            active: root.editingSide === "sell"
            // Sell amount is sent to the backend as a raw base-units integer
            // string; reject fractional entry rather than fail opaquely.
            digitsOnly: true
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
            token: root.buyToken
            active: root.editingSide === "buy"
            onInputEdited: function(v) {
                root.buyInput = v
                if (root.editingSide !== "buy") root.editingSide = "buy"
            }
            onTokenClicked: root.requestTokenSelect("buy")
        }

        Text {
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            visible: root.poolStatusText.length > 0
            text: root.poolStatusText
            color: (root.poolError.length > 0 || (root.poolResolved && !root.poolExists))
                   ? "#F08A76" : theme.colors.textSecondary
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            visible: root.swapError.length > 0
            text: root.swapError
            color: "#F08A76"
            font.pixelSize: 12
            wrapMode: Text.WordWrap
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
