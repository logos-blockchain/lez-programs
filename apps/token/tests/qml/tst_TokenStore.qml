import QtQuick
import QtTest

import "../../qml/state"

Item {
    id: root

    Component {
        id: storeComponent

        TokenStore {}
    }

    TestCase {
        name: "TokenStore"
        when: windowShown

        function createStore() {
            const store = createTemporaryObject(storeComponent, root)
            verify(store, "Component exists")
            return store
        }

        function test_base58AndHexDefinitionsShareOneRow() {
            const store = createStore()
            const definitionHex = "00".repeat(32)
            const definitionBase58 = "1".repeat(32)

            compare(
                store.canonicalIdentifier("9JDLE5Qr8dXKBstucN5sZi5tCCYy7SnfCEKax77JZTd7"),
                "hex:7b464ff9dd0d3bc07f7e2e0b0667ccd066d85ad12be4c79fc55687a863910aa6")

            store.setLiveDefinitions([{
                id: definitionBase58,
                definitionId: definitionBase58,
                name: "Confirmed",
                source: "network"
            }])
            store.addDraft({
                id: definitionHex,
                definitionId: definitionHex,
                definition: { hex: definitionHex },
                name: "Pending",
                source: "pending",
                transactionId: "transaction-1"
            })

            compare(store.allDefinitions.length, 1)
            compare(store.allDefinitions[0].id, definitionBase58)
            compare(store.allDefinitions[0].transactionId, "transaction-1")
            compare(store.findDefinition(definitionHex).id, definitionBase58)
            compare(store.findDefinition(definitionBase58).id, definitionBase58)

            store.addDraft({
                id: definitionHex,
                definitionId: definitionHex,
                source: "pending",
                transactionId: "transaction-1"
            })
            compare(store.allDefinitions.length, 1)
        }

        function test_distinctDefinitionIdsRemainSeparate() {
            const store = createStore()
            const firstDefinition = "00".repeat(32)
            const secondDefinition = "ff".repeat(32)

            store.setLiveDefinitions([])
            store.addDraft({ id: firstDefinition, definitionId: firstDefinition, source: "pending" })
            store.addDraft({ id: secondDefinition, definitionId: secondDefinition, source: "pending" })

            compare(store.allDefinitions.length, 2)
            compare(store.findDefinition(firstDefinition).id, firstDefinition)
            compare(store.findDefinition(secondDefinition).id, secondDefinition)
        }

        function test_duplicateLiveRowsAreCoalesced() {
            const store = createStore()
            const definitionHex = "00".repeat(32)

            store.setLiveDefinitions([
                { id: "11111111111111111111111111111111", source: "network" },
                { id: definitionHex.toUpperCase(), definitionId: definitionHex, source: "network" }
            ])

            compare(store.allDefinitions.length, 1)
            compare(store.allDefinitions[0].id, "11111111111111111111111111111111")
        }
    }
}
