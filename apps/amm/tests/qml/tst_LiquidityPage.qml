pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/pages" as Pages

TestCase {
    id: testCase

    name: "LiquidityPage"

    Component {
        id: backendComponent

        QtObject {
            property bool walletStateReady: false
            property int contextRefreshCalls: 0
            property var newPositionContext: ({
                "status": "ready",
                "tokens": [],
                "feeTiers": []
            })

            function refreshNewPositionContext(request) {
                ++contextRefreshCalls
                return newPositionContext
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
                if (value === undefined)
                    return
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

    function test_refreshPositionCompletesAndReenablesTokenSelection() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "walletStateReady": true
        })
        var runtime = createTemporaryObject(runtimeComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "runtime": runtime
        })
        verify(backend)
        verify(runtime)
        verify(page)

        var refreshButton = findChild(page, "refreshPositionButton")
        var tokenAInput = findChild(page, "tokenAAmountInput")
        verify(refreshButton)
        verify(tokenAInput)

        refreshButton.clicked()

        tryCompare(backend, "contextRefreshCalls", 1)
        tryCompare(page.flow, "contextLoading", false)
        compare(refreshButton.enabled, true)
        compare(tokenAInput.tokenSelectionEnabled, true)
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

}
