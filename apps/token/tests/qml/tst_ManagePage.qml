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
            property bool initialSync: false
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
