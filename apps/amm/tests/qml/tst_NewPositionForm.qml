pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/components/liquidity" as Liquidity

TestCase {
    id: testCase

    name: "NewPositionForm"

    readonly property string tokenLow: "22222222222222222222222222222222"
    readonly property string tokenHigh: "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
    readonly property string tokenThird: "33333333333333333333333333333333"
    readonly property string submittedTransactionId:
        "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"

    Component {
        id: formComponent

        Liquidity.NewPositionForm {
            visible: false
            width: 760
            newPositionContext: testCase.readyContext()
        }
    }

    Component {
        id: clipboardSinkComponent

        TextEdit {}
    }

    SignalSpy {
        id: quoteRequestedSpy
        signalName: "quoteRequested"
    }

    function readyContext() {
        return {
            "status": "ready",
            "tokens": [
                {
                    "definitionId": tokenLow,
                    "name": "Low",
                    "totalSupplyRaw": "1000000",
                    "balanceRaw": "1000",
                    "selectable": true
                },
                {
                    "definitionId": tokenHigh,
                    "name": "High",
                    "totalSupplyRaw": "1000000000000",
                    "balanceRaw": "5000000000",
                    "selectable": true
                }
            ]
        }
    }

    function rawAmountsContext() {
        return {
            "status": "ready",
            "tokens": [
                {
                    "definitionId": tokenLow,
                    "name": "Sir Mints-a-Lot",
                    "totalSupplyRaw": "1000000000000",
                    "balanceRaw": "1000000000",
                    "selectable": true
                },
                {
                    "definitionId": tokenHigh,
                    "name": "Aurora",
                    "totalSupplyRaw": "1000000000000",
                    "balanceRaw": "1000000000",
                    "selectable": true
                }
            ]
        }
    }

    function flowState(quote) {
        return {
            "quote": quote || ({}),
            "contextLoading": false,
            "quoteLoading": false,
            "quoteStale": false,
            "submitting": false
        }
    }

    function createEmptyForm(context) {
        var form = createTemporaryObject(formComponent, testCase, {
            "flowState": flowState(({})),
            "newPositionContext": context || readyContext()
        })
        verify(form)
        wait(0)
        return form
    }

    function createForm(context) {
        var form = createEmptyForm(context)
        form.selectToken("A", tokenLow)
        form.selectToken("B", tokenHigh)
        compare(form.selectedTokenAId, tokenLow)
        compare(form.selectedTokenBId, tokenHigh)
        return form
    }

    function test_walletTokensDoNotPrefillPosition() {
        var form = createEmptyForm()
        compare(form.selectedTokenAId, "")
        compare(form.selectedTokenBId, "")
        compare(form.amountA, "")
        compare(form.amountB, "")
    }

    function test_displayOrderMapsToCanonicalRequestAfterSwap() {
        var form = createForm()
        var built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.tokenAId, tokenHigh)
        compare(built.request.tokenBId, tokenLow)

        form.swapTokens()
        compare(form.selectedTokenAId, tokenHigh)
        compare(form.selectedTokenBId, tokenLow)
        built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.tokenAId, tokenHigh)
        compare(built.request.tokenBId, tokenLow)
    }

    function test_tokenAmountsUseRawUnits() {
        var form = createForm()
        compare(form.decimalsA, 0)
        compare(form.decimalsB, 0)
        compare(form.balanceText(form.tokenA, form.decimalsA), "1000")
        compare(form.balanceText(form.tokenB, form.decimalsB), "5000000000")
    }

    function test_missingPoolKeepsMinimumRatioWhileEditingDeposit() {
        var quote = {
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "missing_pool",
            "minimumAmountARaw": "2",
            "minimumAmountBRaw": "3",
            "actualAmountARaw": "2",
            "actualAmountBRaw": "3"
        }
        var form = createForm()
        form.priceAmountA = "3"
        form.priceAmountB = "2"
        form.flowState = flowState(quote)
        wait(0)
        compare(form.amountA, "3")
        compare(form.amountB, "2")

        form.editMissingAmount("A", "6")
        compare(form.amountA, "6")
        compare(form.amountB, "2")

        form.finishMissingAmount("A", "6")
        compare(form.amountB, "4")

        var built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.amountARaw, "4")
        compare(built.request.amountBRaw, "6")
    }

    function test_missingPoolAcceptsLargeDirectAmountsFromEitherSide() {
        var form = createForm(rawAmountsContext())
        form.priceAmountA = "15"
        form.priceAmountB = "10"
        form.flowState = flowState({
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "missing_pool",
            "minimumAmountARaw": "26",
            "minimumAmountBRaw": "39",
            "actualAmountARaw": "26",
            "actualAmountBRaw": "39"
        })
        wait(0)

        form.finishMissingAmount("A", "150")
        compare(form.amountA, "150")
        compare(form.amountB, "100")
        var built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.amountARaw, "100")
        compare(built.request.amountBRaw, "150")
        compare(built.request.initialPriceRealRaw, "27670116110564327424")
        verify(!built.request.hasOwnProperty("depositScaleBps"))

        form.finishMissingAmount("B", "200")
        compare(form.amountA, "300")
        compare(form.amountB, "200")
        built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.amountARaw, "200")
        compare(built.request.amountBRaw, "300")
    }

    function test_missingPoolRoundsPairedRawAmounts() {
        var form = createForm(rawAmountsContext())
        form.priceAmountA = "15"
        form.priceAmountB = "10"
        form.flowState = flowState({
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "missing_pool",
            "minimumAmountARaw": "1",
            "minimumAmountBRaw": "1",
            "actualAmountARaw": "1",
            "actualAmountBRaw": "1"
        })
        wait(0)

        var cases = [
            { "input": "1", "paired": "1", "rawA": "1", "rawB": "1" },
            { "input": "2", "paired": "1", "rawA": "1", "rawB": "2" },
            { "input": "3", "paired": "2", "rawA": "2", "rawB": "3" },
            { "input": "4", "paired": "3", "rawA": "3", "rawB": "4" }
        ]
        for (var i = 0; i < cases.length; ++i) {
            form.finishMissingAmount("A", cases[i].input)
            compare(form.amountB, cases[i].paired)
            var built = form.buildQuoteRequest()
            verify(built.ok)
            compare(built.request.amountARaw, cases[i].rawA)
            compare(built.request.amountBRaw, cases[i].rawB)
        }

        form.finishMissingAmount("A", "1.1234567")
        compare(form.amountA, "1.1234567")
        verify(!form.buildQuoteRequest().ok)
        compare(form.fieldError("amountA"), form.issueText("invalid_amount_precision"))
    }

    function test_missingPoolUsesTradeStyleInputsWithInlinePrice() {
        var form = createForm()
        form.flowState = flowState({
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "missing_pool",
            "minimumAmountARaw": "2000000",
            "minimumAmountBRaw": "3",
            "actualAmountARaw": "2000000",
            "actualAmountBRaw": "3"
        })
        wait(0)

        var amountAInput = findChild(form, "tokenAAmountInput")
        var amountBInput = findChild(form, "tokenBAmountInput")
        verify(amountAInput)
        verify(amountBInput)
        compare(amountAInput.selectedTokenId, tokenLow)
        compare(amountBInput.selectedTokenId, tokenHigh)
        // missing_pool now renders the holding-selector footer, so the input card grows by the
        // footer's own height plus its extra bottom spacing (the surface adds footerHeight + 30
        // when a footer is active vs + 24 without one, i.e. footerHeight + 6 over the pre-footer
        // bound). Keep the compactness guard, but make it footer-aware.
        verify(amountAInput.footerActive) // the holding-selector footer is present in missing_pool
        verify(amountBInput.footerActive)
        verify(amountAInput.height <= 114 + amountAInput.footerHeight + 6)
        verify(amountBInput.height <= 114 + amountBInput.footerHeight + 6)
        verify(findChild(form, "priceAmountAField"))
        verify(findChild(form, "priceAmountBField"))

        form.width = 328
        wait(0)
        compare(amountAInput.adjustmentWidth, 100)
        compare(amountBInput.adjustmentWidth, 100)
    }

    function test_errorMessageStaysAbovePairAndMarksCausingControl() {
        var form = createForm()
        form.flowState = flowState({
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "missing_pool",
            "minimumAmountARaw": "2000000",
            "minimumAmountBRaw": "3",
            "actualAmountARaw": "2000000",
            "actualAmountBRaw": "3"
        })
        wait(0)
        var amountAInput = findChild(form, "tokenAAmountInput")
        var amountBInput = findChild(form, "tokenBAmountInput")

        form.localErrors = [form.localIssue("amount_exceeds_balance", ["amountB"])]
        wait(0)

        compare(amountAInput.errorText, form.issueText("amount_exceeds_balance"))
        compare(amountAInput.invalid, false)
        compare(amountBInput.invalid, true)

        form.resolveToken("B", "invalid")
        wait(0)
        compare(amountAInput.errorText, form.issueText("invalid_token_id"))
        compare(amountAInput.tokenInvalid, false)
        compare(amountBInput.tokenInvalid, true)
    }

    function test_staleMissingPoolQuoteDoesNotReplaceDraft() {
        var quote = {
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "missing_pool",
            "minimumAmountARaw": "2000000",
            "minimumAmountBRaw": "3",
            "actualAmountARaw": "2000000",
            "actualAmountBRaw": "3"
        }
        var form = createForm()
        form.flowState = flowState(quote)
        wait(0)
        compare(form.amountA, "3")

        form.editMissingAmount("A", "2")
        form.flowState = {
            "quote": {
                "status": "ok",
                "tokenAId": tokenHigh,
                "tokenBId": tokenLow,
                "poolStatus": "missing_pool",
                "minimumAmountARaw": "2000000",
                "minimumAmountBRaw": "3",
                "actualAmountARaw": "2000000",
                "actualAmountBRaw": "3"
            },
            "contextLoading": false,
            "quoteLoading": false,
            "quoteStale": true,
            "submitting": false
        }
        wait(0)

        compare(form.amountA, "2")
    }

    function test_staleQuoteErrorsDoNotMarkCurrentDraft() {
        var quote = {
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "active_pool",
            "reserveARaw": "2",
            "reserveBRaw": "10",
            "maxAmountARaw": "4",
            "maxAmountBRaw": "20",
            "errors": [{
                "code": "amount_exceeds_balance",
                "blockingFields": ["maxAmountARaw"]
            }]
        }
        var form = createForm()
        form.flowState = flowState(quote)
        wait(0)
        compare(form.fieldError("amountB"), form.issueText("amount_exceeds_balance"))

        var staleState = flowState(quote)
        staleState.quoteStale = true
        form.flowState = staleState
        wait(0)

        compare(form.fieldError("amountB"), "")
        compare(form.formErrorText(), "")
        compare(form.accountPreview().length, 0)
    }

    function test_activePoolEditUsesDisplayReserveRatio() {
        var quote = {
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "active_pool",
            "reserveARaw": "2",
            "reserveBRaw": "10",
            "maxAmountARaw": "4",
            "maxAmountBRaw": "20"
        }
        var form = createForm()
        form.flowState = flowState(quote)
        wait(0)
        compare(form.amountA, "")
        compare(form.amountB, "")

        quoteRequestedSpy.target = form
        quoteRequestedSpy.clear()
        form.editActiveAmount("A", "5")
        compare(form.amountA, "5")
        compare(form.amountB, "")
        compare(quoteRequestedSpy.count, 0)

        form.finishActiveAmount("A", "5")
        compare(form.amountB, "1")
        compare(quoteRequestedSpy.count, 1)

        var built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.maxAmountARaw, "1")
        compare(built.request.maxAmountBRaw, "5")

        form.finishActiveAmount("B", "1.1234567")
        compare(form.amountB, "1.1234567")
        verify(!form.buildQuoteRequest().ok)
    }

    function test_activePoolEditRecoversAfterInvalidQuote() {
        var form = createForm()
        form.flowState = flowState({
            "status": "ok",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "poolStatus": "active_pool",
            "poolFeeBps": 30,
            "reserveARaw": "2",
            "reserveBRaw": "10",
            "maxAmountARaw": "4",
            "maxAmountBRaw": "20"
        })
        wait(0)

        form.amountA = "0"
        form.amountB = "0"
        form.flowState = flowState({
            "status": "error",
            "code": "value_must_be_positive",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow
        })
        wait(0)

        compare(form.poolFeeBps, 30)
        form.finishActiveAmount("B", "1")
        compare(form.amountA, "5")

        var built = form.buildQuoteRequest()
        verify(built.ok)
        compare(built.request.maxAmountARaw, "1")
        compare(built.request.maxAmountBRaw, "5")
    }

    function test_quoteStateChangeDoesNotRequestAnotherQuote() {
        var form = createForm()
        quoteRequestedSpy.target = form
        quoteRequestedSpy.clear()

        form.flowState = {
            "quote": ({}),
            "contextLoading": false,
            "quoteLoading": true,
            "quoteStale": true,
            "submitting": false
        }
        wait(0)

        compare(quoteRequestedSpy.count, 0)
    }

    function test_existingPoolFeeCorrectionKeepsQuoteRequestValid() {
        var form = createForm()
        quoteRequestedSpy.target = form
        quoteRequestedSpy.clear()

        form.flowState = flowState({
            "status": "error",
            "code": "fee_tier_mismatch",
            "tokenAId": tokenHigh,
            "tokenBId": tokenLow,
            "errors": [{
                "code": "fee_tier_mismatch",
                "details": { "poolFeeBps": "5" }
            }]
        })
        wait(0)

        compare(form.selectedFeeBps, 5)
        compare(form.amountA, "")
        compare(form.amountB, "")
        compare(quoteRequestedSpy.count, 1)
        verify(quoteRequestedSpy.signalArguments[0][1].ok)
        compare(quoteRequestedSpy.signalArguments[0][1].request.maxAmountARaw, "5000000000")
        compare(quoteRequestedSpy.signalArguments[0][1].request.maxAmountBRaw, "1000")
    }

    function test_contextFailureFinishesTokenResolution() {
        var form = createForm()
        form.resolvingTokenId = tokenThird
        form.resolvingTokenSide = "A"

        form.newPositionContext = {
            "status": "error",
            "code": "config_unavailable",
            "tokens": []
        }
        form.finishTokenResolution(true)
        wait(0)

        compare(form.resolvingTokenId, "")
        compare(form.resolvingTokenSide, "")
        compare(form.tokenResolutionError, form.issueText("config_unavailable"))
    }

    function test_staleContextDoesNotFinishNewerTokenResolution() {
        var form = createForm()
        form.resolvingTokenId = tokenThird
        form.resolvingTokenSide = "B"

        form.newPositionContext = {
            "status": "ready",
            "tokens": [{
                "definitionId": tokenLow,
                "name": "Earlier token",
                "selectable": true
            }]
        }
        wait(0)

        compare(form.resolvingTokenId, tokenThird)
        compare(form.resolvingTokenSide, "B")
        compare(form.tokenResolutionError, "")
    }

    function test_replacingSelectedTokensClearsPairDraft() {
        var form = createForm()
        form.amountA = "12"
        form.amountB = "34"
        form.minimumAmountARaw = "12"
        form.minimumAmountBRaw = "34"
        form.confirmedPoolStatus = "active_pool"

        form.newPositionContext = {
            "status": "ready",
            "tokens": [
                {
                    "definitionId": tokenHigh,
                    "name": "High",
                    "totalSupplyRaw": "1000000000000",
                    "balanceRaw": "5000000000",
                    "selectable": true
                },
                {
                    "definitionId": tokenThird,
                    "name": "Third",
                    "totalSupplyRaw": "1000000",
                    "balanceRaw": "100",
                    "selectable": true
                }
            ]
        }
        wait(0)

        compare(form.selectedTokenAId, "")
        compare(form.selectedTokenBId, tokenHigh)
        compare(form.amountA, "")
        compare(form.amountB, "")
        compare(form.minimumAmountARaw, "")
        compare(form.minimumAmountBRaw, "")
        compare(form.confirmedPoolStatus, "")
    }

    function test_networkFailurePreservesPairDraft() {
        var form = createForm()
        form.amountA = "12"
        form.amountB = "34"
        form.confirmedPoolStatus = "active_pool"

        form.newPositionContext = {
            "status": "network_mismatch",
            "tokens": []
        }
        wait(0)

        compare(form.selectedTokenAId, tokenLow)
        compare(form.selectedTokenBId, tokenHigh)
        compare(form.amountA, "12")
        compare(form.amountB, "34")
        compare(form.confirmedPoolStatus, "active_pool")
        verify(form.contextBlocksForm())
    }

    function test_submittedBase58TransactionIdIsCopied() {
        var state = flowState(({}))
        state.transactionId = submittedTransactionId
        var form = createTemporaryObject(formComponent, testCase, {
            "flowState": state
        })
        var sink = createTemporaryObject(clipboardSinkComponent, testCase)
        verify(form)
        verify(sink)
        wait(0)

        compare(form.transactionId, submittedTransactionId)
        var copyButton = findChild(form, "copySubmittedTransactionButton")
        verify(copyButton)
        copyButton.clicked()
        verify(copyButton.copied)
        sink.paste()
        tryCompare(sink, "text", submittedTransactionId)
    }
}
