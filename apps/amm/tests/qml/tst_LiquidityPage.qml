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
            property bool walletCanSubmit: true
            property bool isWalletOpen: true
            property string walletSyncStatus: "ready"
            property var submitResult: ({})
            property var quoteResult: ({
                "schema": "new-position.v2",
                "status": "ok",
                "poolStatus": "missing_pool"
            })
            property var newPositionQuoteResult: ({})
            property var newPositionSubmitResult: ({})
            property bool deferSubmitResult: false
            property var newPositionContext: ({
                "schema": "new-position.v2",
                "status": "ready",
                "tokens": [],
                "feeTiers": []
            })
            property int contextRefreshCalls: 0
            property int contextRequestId: 0
            property int quoteCalls: 0
            property var quotePoolProbeFlags: []
            property int submitCalls: 0
            property var lastContextRefreshRequest: ({})

            function requestNewPositionSubmit(request, quoteHash, requestId) {
                ++submitCalls
                var result = JSON.parse(JSON.stringify(submitResult || ({})))
                result.requestId = requestId
                if (!deferSubmitResult)
                    newPositionSubmitResult = result
            }

            function requestNewPositionQuote(request, requestId, forceRefresh,
                                             isPoolProbe) {
                ++quoteCalls
                quotePoolProbeFlags = quotePoolProbeFlags.concat([
                    isPoolProbe === true
                ])
                var result = JSON.parse(JSON.stringify(quoteResult || ({})))
                result.requestId = requestId
                newPositionQuoteResult = result
            }

            function refreshNewPositionContext(request) {
                ++contextRefreshCalls
                lastContextRefreshRequest = request
                var result = JSON.parse(JSON.stringify(newPositionContext))
                result.requestId = ++contextRequestId
                newPositionContext = result
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

    function test_positionLayoutUsesDesktopRailAndCompactProgress() {
        var page = createTemporaryObject(pageComponent, testCase)
        verify(page)
        page.visible = true
        wait(0)

        var rail = findChild(page, "positionStepRail")
        var compactSteps = findChild(page, "compactPositionSteps")
        var form = findChild(page, "newPositionForm")
        var refresh = findChild(page, "refreshPositionButton")
        verify(rail)
        verify(compactSteps)
        verify(form)
        verify(refresh)
        compare(refresh.text, "\u21bb")
        compare(page.wideLayout, true)
        verify(form.width > rail.width)

        page.width = 600
        wait(0)
        compare(page.wideLayout, false)
        verify(compactSteps.implicitHeight > 0)
        verify(form.width <= page.width - 32)
    }

    function test_contextAvailableWhileWalletSyncs() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletCanSubmit": false,
            "walletSyncStatus": "syncing"
        })
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        verify(page)

        compare(page.flow.newPositionContext.status, "ready")
        compare(page.flow.viewState.walletSyncStatus, "syncing")
        verify(!page.flow.walletCanSubmit)
    }

    function test_contextRefreshControlsPublicRefresh() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        verify(page)

        compare(page.flow.contextHints(false).refreshWalletAccounts, false)
        compare(page.flow.contextHints(true).refreshWalletAccounts, true)
    }

    function test_refreshControlRequestsContext() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        var refresh = findChild(page, "refreshPositionButton")
        verify(refresh)

        refresh.clicked()
        tryCompare(backend, "contextRefreshCalls", 1)
    }

    function test_repeatedIdenticalContextCompletesRefresh() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        verify(page)

        page.flow.refreshContext(false)
        tryCompare(page.flow, "contextLoading", false)
        compare(backend.contextRequestId, 1)

        page.flow.refreshContext(false)
        tryCompare(page.flow, "contextLoading", false)
        compare(backend.contextRequestId, 2)
        compare(page.flow.newPositionContext.status, "ready")
    }

    function test_submitFailureKeepsReturnedFreshQuoteWithoutRequery() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        page.flow.quoteSerial = 7
        page.flow.finishSubmitFailure({
            "schema": "new-position.v2",
            "status": "error",
            "code": "quote_not_submittable",
            "quote": {
                "schema": "new-position.v2",
                "status": "ok",
                "canSubmit": false,
                "quoteHash": "sha256:fresh"
            }
        })

        compare(page.flow.quoteSerial, 7)
        compare(page.flow.newPositionQuote.quoteHash, "sha256:fresh")
        compare(page.flow.quoteStale, false)
    }

    function test_submittedResultEntersSuccessState() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "submitResult": {
                "schema": "new-position.v2",
                "status": "submitted",
                "transactionId": submittedTransactionId,
                "deadlineMs": String(Date.now() + 60000),
                "affectedAccountIds": []
            }
        })
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        page.flow.confirm({ "request": ({}), "quoteHash": "sha256:expected" })
        wait(0)

        compare(page.flow.transactionId, submittedTransactionId)
        compare(page.flow.flowErrorCode, "")
        compare(page.flow.submitting, false)
        compare(backend.submitCalls, 1)
    }

    function test_submissionCompletionClosesConfirmationWithoutBindingLoop() {
        failOnWarning(/Binding loop detected for property "busy"/)

        var backend = createTemporaryObject(backendComponent, testCase, {
            "deferSubmitResult": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "visible": true
        })
        verify(page)

        var dialog = findChild(page, "liquidityConfirmationDialog")
        verify(dialog)
        dialog.openWithSnapshot({
            "quoteReady": true,
            "request": ({}),
            "quoteHash": "sha256:expected"
        })
        tryCompare(dialog, "opened", true)

        dialog.confirm()
        tryCompare(page.flow, "submitting", true)
        compare(dialog.busy, true)

        backend.newPositionSubmitResult = {
            "schema": "new-position.v2",
            "status": "submitted",
            "transactionId": submittedTransactionId,
            "deadlineMs": String(Date.now() + 60000),
            "affectedAccountIds": [],
            "requestId": page.flow.submitRequestId
        }

        tryCompare(page.flow, "submitting", false)
        tryCompare(dialog, "opened", false)
    }

    function test_submissionFailureKeepsConfirmationOpenForRetry() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "deferSubmitResult": true
        })
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "visible": true
        })
        verify(page)

        var dialog = findChild(page, "liquidityConfirmationDialog")
        verify(dialog)
        dialog.openWithSnapshot({
            "quoteReady": true,
            "request": ({}),
            "quoteHash": "sha256:expected"
        })
        tryCompare(dialog, "opened", true)

        dialog.confirm()
        tryCompare(page.flow, "submitting", true)

        backend.newPositionSubmitResult = {
            "schema": "new-position.v2",
            "status": "error",
            "code": "wallet_submission_failed",
            "quote": {
                "schema": "new-position.v2",
                "status": "ok",
                "canSubmit": true,
                "quoteHash": "sha256:retry"
            },
            "requestId": page.flow.submitRequestId
        }

        tryCompare(page.flow, "submitting", false)
        tryCompare(dialog, "opened", true)
        compare(dialog.confirmationPending, false)
    }

    function test_destinationRequoteUsesQuoteBusyText() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "visible": true
        })
        verify(page)

        var dialog = findChild(page, "liquidityConfirmationDialog")
        verify(dialog)
        dialog.openWithSnapshot({
            "quoteReady": false,
            "request": ({})
        })
        tryCompare(dialog, "opened", true)

        page.flow.quoteLoading = true
        tryCompare(dialog, "busy", true)
        compare(dialog.busyText, "Updating quote…")
        compare(findChild(dialog, "transactionConfirmButton").text,
                "Updating quote…")
        compare(backend.submitCalls, 0)

        page.flow.quoteLoading = false
        dialog.cancel()
    }

    function test_missingPoolSubmissionStartsPoolProbeWithoutWalletRefresh() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "submitResult": {
                "schema": "new-position.v2",
                "status": "submitted",
                "transactionId": submittedTransactionId,
                "deadlineMs": String(Date.now() + 60000)
            }
        })
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        var probe = {
            "tokenAId": "22222222222222222222222222222222",
            "tokenBId": "33333333333333333333333333333333"
        }
        page.flow.pendingQuoteRequest = {
            "ok": true,
            "request": probe
        }
        page.flow.confirm({
            "instruction": "NewDefinition",
            "request": {
                "tokenAId": probe.tokenAId,
                "tokenBId": probe.tokenBId,
                "initialPriceRealRaw": "18446744073709551616"
            },
            "poolProbeRequest": probe,
            "quoteHash": "sha256:expected"
        })
        wait(0)

        compare(page.flow.pendingPoolProbes.length, 1)
        compare(backend.contextRefreshCalls, 0)
        compare(backend.quoteCalls, 0)
        verify(page.flow.selectedPoolCreationPending())

        page.flow.pollPendingPool()
        compare(backend.quoteCalls, 0)

        page.flow.active = true
        wait(0)
        page.flow.pollPendingPool()
        compare(backend.quoteCalls, 1)
        compare(backend.quotePoolProbeFlags.length, 1)
        compare(backend.quotePoolProbeFlags[0], true)
    }

    function test_userQuoteUsesInteractiveRequestLane() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        page.flow.pendingQuoteRequest = {
            "ok": true,
            "request": ({})
        }
        page.flow.active = true
        page.flow.requestQuoteNow(page.flow.quoteSerial)

        verify(backend.quoteCalls > 0)
        compare(backend.quotePoolProbeFlags[0], false)
    }

    function test_activePoolSubmissionDoesNotStartPoolCreationProbe() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "submitResult": {
                "schema": "new-position.v2",
                "status": "submitted",
                "transactionId": submittedTransactionId,
                "deadlineMs": String(Date.now() + 60000)
            }
        })
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        page.flow.confirm({
            "instruction": "AddLiquidity",
            "request": ({}),
            "poolProbeRequest": {
                "tokenAId": "22222222222222222222222222222222",
                "tokenBId": "33333333333333333333333333333333"
            },
            "quoteHash": "sha256:expected"
        })
        wait(0)

        compare(page.flow.pendingPoolProbes.length, 0)
    }

    function test_walletSyncDisablesSubmissionOnly() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletCanSubmit": false,
            "walletSyncStatus": "syncing"
        })
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        page.flow.confirm({ "request": ({}), "quoteHash": "sha256:expected" })

        compare(backend.submitCalls, 0)
        compare(page.flow.flowErrorCode, "wallet_syncing")
        verify(!page.flow.submitting)
    }

    function test_backendLossEndsSubmissionWithUnknownStatus() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "deferSubmitResult": true,
            "submitResult": {
                "schema": "new-position.v2",
                "status": "submitted",
                "transactionId": submittedTransactionId
            }
        })
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        verify(page)

        page.flow.confirm({ "request": ({}), "quoteHash": "sha256:expected" })
        const staleRequestId = page.flow.submitRequestId
        verify(page.flow.submitting)
        verify(staleRequestId > 0)

        page.backend = null
        tryCompare(page.flow, "submitting", false)
        compare(page.flow.submitRequestId, 0)
        compare(page.flow.flowErrorCode, "submission_status_unknown")

        page.backend = backend
        backend.newPositionSubmitResult = {
            "schema": "new-position.v2",
            "status": "submitted",
            "transactionId": submittedTransactionId,
            "requestId": staleRequestId
        }
        wait(0)
        compare(page.flow.transactionId, "")
        compare(page.flow.flowErrorCode, "submission_status_unknown")
    }

    function test_poolProbeDoesNotPublishProbeAsCurrentQuote() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, { "backend": backend })
        var request = {
            "tokenAId": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            "tokenBId": "22222222222222222222222222222222"
        }
        var pending = { "key": page.flow.pairKey(request), "request": request }
        page.flow.pendingQuoteRequest = { "ok": true, "request": request }
        page.flow.pendingPoolProbes = [pending]
        page.flow.newPositionQuote = {
            "schema": "new-position.v2",
            "status": "ok",
            "poolStatus": "missing_pool"
        }
        page.flow.quoteStale = false

        page.flow.finishPoolProbe(pending, {
            "schema": "new-position.v2",
            "status": "ok",
            "poolStatus": "active_pool"
        })

        compare(page.flow.newPositionQuote.poolStatus, "missing_pool")
        verify(page.flow.quoteStale)
    }

    function test_activePoolDiscoveryDoesNotExposeProbeAsSubmittableQuote() {
        var tokenA = "22222222222222222222222222222222"
        var tokenB = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        var page = createTemporaryObject(pageComponent, testCase)
        verify(page)
        var activated = null
        function captureActivation(quote) {
            activated = quote
        }
        page.flow.poolActivated.connect(captureActivation)
        page.flow.pendingQuoteRequest = {
            "ok": true,
            "poolDiscoveryProbe": true,
            "request": {
                "tokenAId": tokenA,
                "tokenBId": tokenB
            }
        }
        page.flow.activeQuoteRequestId = 7
        page.flow.quoteLoading = true
        page.flow.quoteStale = false
        page.flow.newPositionQuote = {
            "schema": "new-position.v2",
            "status": "ok",
            "canSubmit": true,
            "quoteHash": "sha256:old"
        }

        page.flow.acceptQuoteResult({
            "schema": "new-position.v2",
            "status": "ok",
            "canSubmit": true,
            "tokenAId": tokenA,
            "tokenBId": tokenB,
            "poolStatus": "active_pool",
            "reserveARaw": "90000000000",
            "reserveBRaw": "90000000000",
            "actualAmountARaw": "90000000000",
            "actualAmountBRaw": "90000000000",
            "expectedLpRaw": "90000000000",
            "quoteHash": "sha256:probe",
            "requestId": 7
        })

        compare(activated.poolStatus, "active_pool")
        compare(page.flow.quoteLoading, false)
        verify(page.flow.quoteStale)
        compare(Object.keys(page.flow.newPositionQuote).length, 0)
        page.flow.poolActivated.disconnect(captureActivation)
    }
}
