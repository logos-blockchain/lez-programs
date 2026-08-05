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

    // ── Exact-input quote (backend.swapExactInQuote) ────────────────────────
    property bool quoteInLoading: false
    property string quoteInError: ""
    property string quoteExpectedOutRaw: "0"
    property string quoteMinReceivedRaw: "0"
    property int quotePriceImpactBps: 0

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

    onSellTokenChanged: { root.requestResolve(); root.requestQuoteIn() }
    onBuyTokenChanged: { root.requestResolve(); root.requestQuoteIn() }
    onSellInputChanged: root.requestQuoteIn()
    onEditingSideChanged: root.requestQuoteIn()
    onSlippageTolerancePercentChanged: root.requestQuoteIn()

    function doResolvePool() {
        if (!root.backend || !root.sellToken || !root.buyToken)
            return

        // Capture the pair this request is for. resolvePool callbacks can arrive
        // out of order: if the user switches tokens while an earlier resolve is
        // still in flight, its (stale) callback must NOT overwrite the current
        // pair's pool state — that would corrupt the preview and the submitted
        // min_out. A stale callback also leaves poolLoading alone, since the
        // newer in-flight request owns it.
        var reqSell = root.sellToken.definitionId
        var reqBuy = root.buyToken.definitionId
        function isStale() {
            return !root.sellToken || !root.buyToken
                || root.sellToken.definitionId !== reqSell
                || root.buyToken.definitionId !== reqBuy
        }

        root.poolLoading = true
        logos.watch(root.backend.resolvePool(reqSell, reqBuy),
            function (pool) {
                if (isStale())
                    return
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
                if (isStale())
                    return
                console.warn("resolvePool error:", error)
                root.poolLoading = false
                root.poolResolved = true
                root.poolExists = false
                root.poolError = qsTr("Failed to resolve pool: %1").arg(error)
            })
    }

    // ── Exact-input quote ──────────────────────────────────────────────────────
    Timer {
        id: quoteInDebounce
        interval: 350
        repeat: false
        onTriggered: root.doQuoteIn()
    }

    function resetQuoteIn() {
        root.quoteExpectedOutRaw = "0"
        root.quoteMinReceivedRaw = "0"
        root.quotePriceImpactBps = 0
    }

    function requestQuoteIn() {
        root.quoteInError = ""
        // Only the exact-input (Sell) direction is server-quoted; the Buy
        // direction is still a local preview (see DummySwapState).
        if (root.backend && root.editingSide === "sell" && root.sellToken && root.buyToken
            && root.parsedSellInput > 0) {
            // Invalidate the previous quote up front: while a re-quote is pending
            // (debounce + in-flight), the stale expected-out / min_out must not be
            // shown or submittable. quoteInLoading gates canSubmit until the fresh
            // quote lands. Gating on backend here avoids setting the loading flag
            // when doQuoteIn would only bail — which would leave it stuck.
            root.resetQuoteIn()
            root.quoteInLoading = true
            quoteInDebounce.restart()
        } else {
            quoteInDebounce.stop()
            root.quoteInLoading = false
            root.resetQuoteIn()
        }
    }

    function doQuoteIn() {
        if (!root.backend || root.editingSide !== "sell"
            || !root.sellToken || !root.buyToken || root.parsedSellInput <= 0) {
            root.quoteInLoading = false
            return
        }

        // Capture the request identity: quote callbacks can arrive out of order,
        // so a stale one (tokens or the typed amount changed since) must not
        // overwrite the current preview or the submitted min_out.
        var reqSell = root.sellToken.definitionId
        var reqBuy = root.buyToken.definitionId
        var reqAmount = root.sellInput
        function isStale() {
            return root.editingSide !== "sell"
                || !root.sellToken || !root.buyToken
                || root.sellToken.definitionId !== reqSell
                || root.buyToken.definitionId !== reqBuy
                || root.sellInput !== reqAmount
        }

        var slippageBps = Math.round(root.slippageTolerancePercent * 100)
        root.quoteInLoading = true
        logos.watch(root.backend.swapExactInQuote(reqSell, reqBuy, reqAmount, slippageBps),
            function (quote) {
                if (isStale())
                    return
                root.quoteInLoading = false
                if (quote && quote.status === "ok") {
                    root.quoteExpectedOutRaw = quote.expectedOutRaw || "0"
                    root.quoteMinReceivedRaw = quote.minReceivedRaw || "0"
                    root.quotePriceImpactBps = quote.priceImpactBps || 0
                    root.quoteInError = ""
                } else {
                    root.resetQuoteIn()
                    // no_pool is surfaced via the pool status text, not as an error.
                    var code = (quote && quote.error) || "backend_error"
                    root.quoteInError = code === "no_pool" ? "" : code
                }
            },
            function (error) {
                if (isStale())
                    return
                console.warn("swapExactInQuote error:", error)
                root.quoteInLoading = false
                root.resetQuoteIn()
                root.quoteInError = String(error)
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

    // Exact-input (Sell) expected output comes from the server quote; the Buy
    // direction still estimates locally. Number() may lose precision on large
    // base-unit values, so this drives gating only — the exact figures shown and
    // submitted come from quoteExpectedOutRaw / quoteMinReceivedRaw directly.
    readonly property real parsedBuyAmount: editingSide === "buy"
        ? parsedBuyInput
        : (Number(root.quoteExpectedOutRaw) || 0)

    readonly property real feeAmount: swapState.feeAmount(parsedSellAmount)
    readonly property real minReceivedAmount: editingSide === "sell"
        ? (Number(root.quoteMinReceivedRaw) || 0)
        : swapState.minReceived(parsedBuyAmount, slippageTolerancePercent)
    readonly property real priceImpactPercent: editingSide === "sell"
        ? root.quotePriceImpactBps / 100
        : swapState.priceImpactPercent(parsedSellAmount, parsedBuyAmount, sellReserveNum, buyReserveNum)

    readonly property string swapModeText: editingSide === "buy" ? qsTr("Exact output (preview only)") : qsTr("Exact input")

    readonly property bool hasAmount: editingSide === "sell" ? parsedSellInput > 0 : parsedBuyInput > 0
    readonly property bool tokensSelected: sellToken !== null && buyToken !== null
    readonly property bool insufficientLiquidity: hasAmount && root.poolExists && parsedBuyAmount > buyReserveNum
    // True only when THIS app's wallet is connected. The backend also enforces
    // this before submitting (AmmUiBackend::swapExactInput), but gate the UI too
    // so a disconnected app never even initiates a swap against the shared wallet.
    readonly property bool walletOpen: root.backend !== null && root.backend.isWalletOpen

    // The backend only exposes swapExactInput, so only the "I know exactly
    // how much I'm selling" direction can actually be submitted. Editing the
    // buy field still previews an estimate (via amountInFor above) but can't
    // be submitted — see doc comment on AmmUiBackend::swapExactInput.
    readonly property bool canSubmit: tokensSelected && editingSide === "sell" && hasAmount
                                       && parsedSellAmount > 0 && parsedBuyAmount > 0
                                       && root.poolResolved && root.poolExists
                                       && !insufficientLiquidity && !root.swapInProgress
                                       && !root.quoteInLoading && root.walletOpen

    readonly property string submitButtonText: {
        if (!tokensSelected) return qsTr("Select tokens")
        if (root.swapInProgress) return qsTr("Submitting…")
        if (!hasAmount) return qsTr("Enter an amount")
        if (editingSide === "buy") return qsTr("Enter a sell amount to swap")
        if (root.poolLoading || !root.poolResolved) return qsTr("Resolving pool…")
        if (!root.poolExists) return qsTr("No pool / no liquidity")
        if (root.quoteInLoading) return qsTr("Quoting…")
        if (insufficientLiquidity) return qsTr("Insufficient liquidity")
        if (parsedBuyAmount <= 0) return qsTr("Amount too small")
        if (!root.walletOpen) return qsTr("Connect wallet to swap")
        return qsTr("Swap")
    }

    readonly property string poolStatusText: {
        if (!root.backend) return qsTr("Wallet backend not ready.")
        if (!tokensSelected) return ""
        if (root.poolLoading) return qsTr("Looking up pool…")
        if (root.poolError.length > 0) return root.poolError
        if (root.poolResolved && !root.poolExists) return qsTr("No pool / no liquidity for this pair.")
        if (root.quoteInError.length > 0) return qsTr("Quote failed: %1").arg(root.quoteInError)
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

    // Sell direction shows the quote's exact-integer expected output verbatim
    // (no double round-trip); Buy direction still renders the local estimate.
    readonly property string buyDisplay: editingSide === "buy"
        ? buyInput
        : ((root.quoteExpectedOutRaw && root.quoteExpectedOutRaw !== "0") ? root.quoteExpectedOutRaw : "")

    // Only reached in the Sell (exact-input) direction — canSubmit gates the CTA
    // to editingSide === "sell" — so the amounts come straight from the raw
    // input and the quote's exact-integer figures.
    function buildSnapshot() {
        return {
            "sellToken": sellToken ? sellToken.symbol : "",
            "buyToken": buyToken ? buyToken.symbol : "",
            "sellAmount": root.sellInput,
            "buyAmount": root.quoteExpectedOutRaw,
            "minReceived": root.quoteMinReceivedRaw,
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

        // The submitted slippage floor is the quote's exact-integer minReceivedRaw
        // (base units), derived server-side from the same formula the chain uses —
        // no client-side reserve orientation or double-precision recompute.
        var minOutStr = root.quoteMinReceivedRaw
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
            inputObjectName: "swapSellInput"
            buttonObjectName: "swapSellTokenButton"
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
            inputObjectName: "swapBuyInput"
            buttonObjectName: "swapBuyTokenButton"
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
            objectName: "swapSubmitButton"
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
