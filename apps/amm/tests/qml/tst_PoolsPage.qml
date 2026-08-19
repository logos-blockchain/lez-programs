pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/pages" as Pages

TestCase {
    id: testCase

    name: "PoolsPage"

    Component {
        id: backendComponent

        QtObject {
            property var poolListResult: [
                {
                    "tokenA": "TKA", "tokenB": "TKB", "feeBps": 5,
                    "tokenADefinitionId": "DEF_A", "tokenBDefinitionId": "DEF_B"
                },
                { "tokenA": "TKC", "tokenB": "TKA", "feeBps": 30 }
            ]

            function poolList() {
                return poolListResult
            }
        }
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

        Pages.PoolsPage {
            visible: false
            width: 800
            height: 600
        }
    }

    function test_rowsAreDrivenByBackendPoolList() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var runtime = createTemporaryObject(runtimeComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "runtime": runtime
        })
        verify(backend)
        verify(runtime)
        verify(page)

        // The config drives the row count — no pair is hardcoded in the page,
        // so adding entries to poolList() is all it takes to render more rows.
        compare(page.poolCount, 2)
        verify(page.feeLabel(5).endsWith("%"))

        var list = findChild(page, "poolsList")
        var firstRow = findChild(page, "poolRow0")
        verify(list)
        verify(firstRow)
        compare(firstRow.pairText, "TKA / TKB")
        compare(firstRow.feeText, page.feeLabel(5))
    }

    function test_activatingARowHandsItsPoolToTheDetailView() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var runtime = createTemporaryObject(runtimeComponent, testCase)
        var page = createTemporaryObject(pageComponent, testCase, {
            "backend": backend,
            "runtime": runtime
        })
        verify(page)

        var spy = createTemporaryObject(spyComponent, testCase, {
            "target": page,
            "signalName": "poolActivated"
        })
        verify(spy)

        findChild(page, "poolRow1").activate()

        // The whole row goes through, so the detail view gets the definition ids
        // it needs to resolve the pool — not just the displayed pair.
        compare(spy.count, 1)
        compare(spy.signalArguments[0][0].tokenA, "TKC")
        compare(spy.signalArguments[0][0].feeBps, 30)

        findChild(page, "poolRow0").activate()
        compare(spy.count, 2)
        compare(spy.signalArguments[1][0].tokenADefinitionId, "DEF_A")
    }

    Component {
        id: spyComponent

        SignalSpy {}
    }

    function test_emptyPoolModelUsesTheEmptyState() {
        var page = createTemporaryObject(pageComponent, testCase)
        verify(page)

        // No backend wired: pools defaults to [] and the empty state shows.
        compare(page.poolCount, 0)
        compare(page.showEmptyState, true)
    }
}
