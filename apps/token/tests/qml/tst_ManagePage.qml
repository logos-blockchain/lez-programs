pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/pages"
import "../../qml/state"

Item {
    id: root

    width: 1200
    height: 900

    Component {
        id: backendComponent

        QtObject {
            property bool isWalletOpen: true
            property string syncStatus: "ready"
            property var liveDefinitions: []
            property int walletDefinitionsCalls: 0

            function walletDefinitions() {
                ++walletDefinitionsCalls
                return liveDefinitions
            }
        }
    }

    Component {
        id: runtimeComponent

        QtObject {
            property int watchCalls: 0

            function watch(result, success, _failure) {
                ++watchCalls
                success(result)
            }
        }
    }

    Component {
        id: storeComponent

        TokenStore {}
    }

    Component {
        id: pageComponent

        ManagePage {
            width: root.width
            height: root.height
        }
    }

    TestCase {
        name: "ManagePage"
        when: windowShown

        readonly property string tokenA: "1".repeat(32)
        readonly property string tokenBHex: "7b464ff9dd0d3bc07f7e2e0b0667ccd066d85ad12be4c79fc55687a863910aa6"
        readonly property string tokenB: "9JDLE5Qr8dXKBstucN5sZi5tCCYy7SnfCEKax77JZTd7"

        function record(id, name, source) {
            return {
                id: id,
                definitionId: id,
                name: name,
                type: "fungible",
                instruction: "createFungible",
                authority: "",
                holdings: [],
                source: source
            }
        }

        function createFixture() {
            const backend = createTemporaryObject(backendComponent, root)
            const runtime = createTemporaryObject(runtimeComponent, root)
            const store = createTemporaryObject(storeComponent, root)
            const page = createTemporaryObject(pageComponent, root, {
                backend: backend,
                runtime: runtime,
                store: store
            })
            verify(backend && runtime && store && page)
            return { backend, runtime, store, page }
        }

        function test_selectedPendingTokenStaysSelectedAfterConfirmation() {
            const fixture = createFixture()
            fixture.store.setLiveDefinitions([record(tokenA, "Token A", "network")])
            const pending = record(tokenBHex, "Token B", "pending")
            pending.transactionId = "transaction-1"
            fixture.store.addDraft(pending)
            fixture.page.selectedId = tokenBHex
            compare(fixture.page.selectedDefinition.name, "Token B")

            fixture.store.setLiveDefinitions([
                record(tokenA, "Token A", "network"),
                record(tokenB, "Token B", "network")
            ])

            compare(fixture.page.selectedDefinition.name, "Token B")
            compare(fixture.page.selectedId, tokenB)
            compare(fixture.page.selectedDefinition.source, "network")
            compare(fixture.page.selectedDefinition.transactionId, "transaction-1")
            compare(fixture.store.allDefinitions.length, 2)
        }

        function test_distinctTokensKeepTheirSelectionOnRefresh() {
            const fixture = createFixture()
            fixture.store.setLiveDefinitions([
                record(tokenB, "Same name", "network"),
                record(tokenA, "Same name", "network")
            ])
            fixture.page.selectedId = tokenB

            fixture.store.setLiveDefinitions([
                record(tokenA, "Same name", "network"),
                record(tokenB, "Same name", "network")
            ])

            compare(fixture.store.allDefinitions.length, 2)
            compare(fixture.page.selectedId, tokenB)
            compare(fixture.page.selectedDefinition.id, tokenB)
        }

        function test_removedTokenSelectsRemainingToken() {
            const fixture = createFixture()
            fixture.store.setLiveDefinitions([
                record(tokenA, "Token A", "network"),
                record(tokenB, "Token B", "network")
            ])
            fixture.page.selectedId = tokenB

            fixture.store.setLiveDefinitions([record(tokenA, "Token A", "network")])

            compare(fixture.page.selectedId, tokenA)
            compare(fixture.page.selectedDefinition.id, tokenA)

            fixture.store.setLiveDefinitions([])

            compare(fixture.page.selectedId, "")
            compare(fixture.page.selectedDefinition, null)
        }

        function test_refreshesAfterWalletSyncWithoutReconnect() {
            const fixture = createFixture()
            const definitionHex = "00".repeat(32)
            const definitionBase58 = "1".repeat(32)

            fixture.store.setLiveDefinitions([])
            fixture.store.addDraft({
                id: definitionHex,
                definitionId: definitionHex,
                name: "Pending",
                type: "fungible",
                instruction: "new_fungible_definition",
                authority: "",
                holdings: [],
                source: "pending",
                transactionId: "transaction-1"
            })
            compare(fixture.store.allDefinitions.length, 1)

            const initialWatchCalls = fixture.runtime.watchCalls
            fixture.backend.liveDefinitions = [{
                id: definitionBase58,
                definitionId: definitionBase58,
                name: "Confirmed",
                type: "fungible",
                instruction: "new_fungible_definition",
                authority: "",
                holdings: [],
                source: "network"
            }]
            fixture.backend.syncStatus = "syncing"
            wait(20)
            compare(fixture.runtime.watchCalls, initialWatchCalls)

            fixture.backend.syncStatus = "ready"
            tryVerify(function() {
                return fixture.store.allDefinitions.length === 1
                    && fixture.store.allDefinitions[0].source === "network"
            })
            compare(fixture.store.allDefinitions[0].id, definitionBase58)
            compare(fixture.runtime.watchCalls, initialWatchCalls + 1)
        }
    }
}
