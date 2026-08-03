pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/pages" as Pages

TestCase {
    id: testCase

    name: "LiquidityPage"
    readonly property string submittedTransactionId:
        "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"

    Component {
        id: backendComponent

        QtObject {
            property bool walletStateReady: false
            property var submitResult: ({})
            property var quoteResult: ({
                "status": "ok",
                "poolStatus": "missing_pool"
            })
            property var newPositionContext: ({
                "status": "ready",
                "tokens": [],
                "feeTiers": []
            })

            function submitNewPosition(request, quoteHash) {
                return submitResult
            }

            function quoteNewPosition(request) {
                return quoteResult
            }
        }
    }

    function test_positionLayoutUsesDesktopRailAndCompactProgress() {
        var page = createTemporaryObject(pageComponent, testCase)
        verify(page)
        page.visible = true
        wait(0)

        var rail = findChild(page, "positionStepRail")
        var compactSteps = findChild(page, "compactPositionSteps")
        var form = findChild(page, "newPositionForm")
        verify(rail)
        verify(compactSteps)
        verify(form)

        compare(page.wideLayout, true)
        verify(rail.width > 0)
        verify(form.width > rail.width)

        page.width = 600
        wait(0)

        compare(page.wideLayout, false)
        verify(compactSteps.implicitHeight > 0)
        verify(form.width > 0)
        verify(form.width <= page.width - 32)
    }

    Component {
        id: runtimeComponent

        QtObject {
            function watch(value, succeeded, failed) {
                succeeded(value)
            }
        }
    }

    Component {
        id: pageComponent

        Pages.LiquidityPage {
            visible: false
            width: 800
            height: 600
        }
    }

    function test_contextWaitsForWalletState() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend
        })
        verify(backend)
        verify(page)

        compare(page.flow.newPositionContext.status, "loading")

        backend.walletStateReady = true
        tryCompare(page.flow.newPositionContext, "status", "ready")
    }

    function test_contextRefreshControlsWalletScan() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend
        })
        verify(backend)
        verify(page)

        compare(page.flow.contextHints(false).refreshWalletAccounts, false)
        compare(page.flow.contextHints(true).refreshWalletAccounts, true)
    }

    function test_staleContextCompletionCannotFinishNewerRefresh() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend
        })
        verify(backend)
        verify(page)

        page.flow.contextSerial = 2
        page.flow.contextLoading = true
        page.flow.contextErrorCode = "newer_request_pending"

        page.flow.finishContextRefresh(1, null)
        page.flow.failContextRefresh(1)

        compare(page.flow.contextLoading, true)
        compare(page.flow.contextErrorCode, "newer_request_pending")

        page.flow.finishContextRefresh(2, null)
        compare(page.flow.contextLoading, false)
        compare(page.flow.contextErrorCode, "")
    }

    function test_submitFailureKeepsReturnedFreshQuoteWithoutRequery() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend
        })
        verify(backend)
        verify(page)

        page.flow.quoteSerial = 7
        page.flow.finishSubmitFailure({
            "status": "error",
            "code": "quote_not_submittable",
            "quote": {
                "status": "ok",
                "canSubmit": false,
                "quoteHash": "sha256:fresh"
            }
        })

        compare(page.flow.quoteSerial, 7)
        compare(page.flow.newPositionQuote.quoteHash, "sha256:fresh")
        compare(page.flow.quoteStale, false)
    }

    function test_base58SubmittedResultEntersSuccessState() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true,
            "submitResult": {
                "status": "submitted",
                "transactionId": submittedTransactionId,
                "deadlineMs": String(Date.now() + 60000)
            }
        })
        var runtime = createTemporaryObject(runtimeComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "runtime": runtime
        })
        verify(backend)
        verify(runtime)
        verify(page)

        page.flow.confirm({
            "request": ({}),
            "quoteHash": "sha256:expected"
        })

        compare(page.flow.transactionId, submittedTransactionId)
        compare(page.flow.flowErrorCode, "")
        compare(page.flow.submitting, false)
    }

    function test_base58MissingPoolSubmissionStartsPoolWatch() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true,
            "submitResult": {
                "status": "submitted",
                "transactionId": submittedTransactionId,
                "deadlineMs": String(Date.now() + 60000)
            }
        })
        var runtime = createTemporaryObject(runtimeComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "runtime": runtime
        })
        verify(backend)
        verify(runtime)
        verify(page)

        var probe = {
            "tokenAId": "22222222222222222222222222222222",
            "tokenBId": "33333333333333333333333333333333"
        }
        page.flow.pendingQuoteRequest = { "ok": true, "request": probe }
        page.flow.confirm({
            "request": {
                "initialPriceRealRaw": "18446744073709551616"
            },
            "poolProbeRequest": probe,
            "quoteHash": "sha256:expected"
        })
        wait(0)

        compare(page.flow.transactionId, submittedTransactionId)
        compare(page.flow.pendingPoolProbes.length, 1)
        compare(page.flow.selectedPoolCreationPending(), true)
        compare(page.flow.poolProbeInFlight, false)
    }

    function test_nativeHexSubmittedResultDoesNotEnterSuccessState() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true,
            "submitResult": {
                "status": "submitted",
                "transactionId": "000102030405060708090a0b0c0d0e0f"
                                 + "101112131415161718191a1b1c1d1e1f"
            }
        })
        var runtime = createTemporaryObject(runtimeComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "runtime": runtime
        })
        verify(backend)
        verify(runtime)
        verify(page)

        page.flow.confirm({
            "request": ({}),
            "quoteHash": "sha256:expected"
        })

        compare(page.flow.transactionId, "")
        compare(page.flow.flowErrorCode, "wallet_submission_failed")
        compare(page.flow.submitting, false)
    }

    function test_poolProbeDoesNotPublishProbeAmountsAsCurrentQuote() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend
        })
        verify(backend)
        verify(page)

        var request = {
            "tokenAId": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            "tokenBId": "22222222222222222222222222222222"
        }
        var pending = { "key": page.flow.pairKey(request), "request": request }
        page.flow.pendingQuoteRequest = { "ok": true, "request": request }
        page.flow.pendingPoolProbes = [pending]
        page.flow.newPositionQuote = {
            "status": "ok",
            "poolStatus": "missing_pool",
            "tokenAId": request.tokenAId,
            "tokenBId": request.tokenBId
        }
        page.flow.quoteStale = false

        page.flow.finishPoolProbe(pending, {
            "status": "ok",
            "poolStatus": "active_pool",
            "tokenAId": request.tokenAId,
            "tokenBId": request.tokenBId
        })

        compare(page.flow.pendingPoolProbes.length, 0)
        compare(page.flow.newPositionQuote.poolStatus, "missing_pool")
        verify(page.flow.quoteStale)
    }

    function test_poolProbeStopsBlockingAfterTransactionDeadline() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend
        })
        verify(backend)
        verify(page)

        var request = {
            "tokenAId": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            "tokenBId": "22222222222222222222222222222222"
        }
        var pending = {
            "key": page.flow.pairKey(request),
            "request": request,
            "deadlineMs": Date.now() - 1
        }
        page.flow.pendingQuoteRequest = { "ok": true, "request": request }
        page.flow.pendingPoolProbes = [pending]
        page.flow.poolProbeInFlight = true

        page.flow.finishPoolProbe(pending, null)

        compare(page.flow.pendingPoolProbes.length, 0)
        compare(page.flow.poolProbeInFlight, false)
        verify(!page.flow.selectedPoolCreationPending())
    }
}
