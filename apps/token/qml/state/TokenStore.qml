import QtQuick 2.15

QtObject {
    id: root

    readonly property string network: "testnet"
    readonly property string tokenProgramId: "F8sGbDbjcxvJHpUQJcArEaY7EbLMVmqZgRm3fXPw3jb3"

    readonly property var fixtureDefinitions: [
        {
            "id": "9JDLE5Qr8dXKBstucN5sZi5tCCYy7SnfCEKax77JZTd7",
            "name": "Pebble",
            "symbol": "",
            "type": "fungible",
            "definitionId": "9JDLE5Qr8dXKBstucN5sZi5tCCYy7SnfCEKax77JZTd7",
            "definitionHex": "7b464ff9dd0d3bc07f7e2e0b0667ccd066d85ad12be4c79fc55687a863910aa6",
            "holdingId": "DhKocL4KzaRbL25Dw3V8rDvTa6aNefxyCAb8F22Tyazn",
            "metadataId": null,
            "rawSupply": "7654321",
            "displaySupply": "7,654,321",
            "inferredDecimals": 0,
            "authorityMode": "fixed",
            "authority": null,
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "9JDLE5Qr8dXKBstucN5sZi5tCCYy7SnfCEKax77JZTd7",
                "hex": "7b464ff9dd0d3bc07f7e2e0b0667ccd066d85ad12be4c79fc55687a863910aa6",
                "name": "Pebble",
                "type": "fungible",
                "totalSupplyRaw": "7654321",
                "mintAuthority": null,
                "metadataId": null
            },
            "holding": {
                "id": "DhKocL4KzaRbL25Dw3V8rDvTa6aNefxyCAb8F22Tyazn",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "6419754",
                "displayBalance": "6,419,754"
            },
            "holdings": [
                {
                    "id": "DhKocL4KzaRbL25Dw3V8rDvTa6aNefxyCAb8F22Tyazn",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "6419754",
                    "displayBalance": "6,419,754"
                },
                {
                    "id": "78Ynj85rpDFfAH2Tk1F296k6uJrU2h8mFoAsJg8sM3DZ",
                    "wallet": "single-asset",
                    "role": "recipient",
                    "rawBalance": "1234567",
                    "displayBalance": "1,234,567"
                }
            ],
            "metadata": null
        },
        {
            "id": "5u7LShcFeBwFubWbD1N6jBYMGonEuj1sfi2A6j9D5UK3",
            "name": "Aurora",
            "symbol": "",
            "type": "fungible",
            "definitionId": "5u7LShcFeBwFubWbD1N6jBYMGonEuj1sfi2A6j9D5UK3",
            "definitionHex": "48c81cf032e601ca367fc9816b957dbf5c0e4c11cf7008e8f4581ec1a67aab42",
            "holdingId": "5rZuJSHTm2NggSBFbKuyZd6D3f7qWAhTB7YsgFZDYe8",
            "metadataId": null,
            "rawSupply": "98765432100",
            "displaySupply": "98,765.4321",
            "inferredDecimals": 6,
            "authorityMode": "fixed",
            "authority": null,
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "5u7LShcFeBwFubWbD1N6jBYMGonEuj1sfi2A6j9D5UK3",
                "hex": "48c81cf032e601ca367fc9816b957dbf5c0e4c11cf7008e8f4581ec1a67aab42",
                "name": "Aurora",
                "type": "fungible",
                "totalSupplyRaw": "98765432100",
                "mintAuthority": null,
                "metadataId": null
            },
            "holding": {
                "id": "5rZuJSHTm2NggSBFbKuyZd6D3f7qWAhTB7YsgFZDYe8",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "90000000000",
                "displayBalance": "90,000"
            },
            "holdings": [
                {
                    "id": "5rZuJSHTm2NggSBFbKuyZd6D3f7qWAhTB7YsgFZDYe8",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "90000000000",
                    "displayBalance": "90,000"
                },
                {
                    "id": "HrC5H28wnK64dsx4xDkUUPipxDvcNFmB9FizJ4vDEHLR",
                    "wallet": "mixed-assets",
                    "role": "recipient",
                    "rawBalance": "8765432100",
                    "displayBalance": "8,765.4321"
                }
            ],
            "metadata": null
        },
        {
            "id": "2TN8jTUxgxRZgATDGANqiNuALpAyUM3Lks6wiNYvSdDi",
            "name": "Cobalt",
            "symbol": "",
            "type": "fungible",
            "definitionId": "2TN8jTUxgxRZgATDGANqiNuALpAyUM3Lks6wiNYvSdDi",
            "definitionHex": "159caef810ea545951b3bd913efe625ee45008c80865c330e72a72ed48b61649",
            "holdingId": "7mKCcw4dtQW6LyxFazZpB7XUisfGLRNfxBcWEiJaBvgZ",
            "metadataId": null,
            "rawSupply": "9876543210000",
            "displaySupply": "9,876,543.21",
            "inferredDecimals": 6,
            "authorityMode": "fixed",
            "authority": null,
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "2TN8jTUxgxRZgATDGANqiNuALpAyUM3Lks6wiNYvSdDi",
                "hex": "159caef810ea545951b3bd913efe625ee45008c80865c330e72a72ed48b61649",
                "name": "Cobalt",
                "type": "fungible",
                "totalSupplyRaw": "9876543210000",
                "mintAuthority": null,
                "metadataId": null
            },
            "holding": {
                "id": "7mKCcw4dtQW6LyxFazZpB7XUisfGLRNfxBcWEiJaBvgZ",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "9876543210000",
                "displayBalance": "9,876,543.21"
            },
            "holdings": [
                {
                    "id": "7mKCcw4dtQW6LyxFazZpB7XUisfGLRNfxBcWEiJaBvgZ",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "9876543210000",
                    "displayBalance": "9,876,543.21"
                }
            ],
            "metadata": null
        },
        {
            "id": "8wRnGribscuUhJJFca4nXwR8iAYtrpUU1JXLqZNXMSou",
            "name": "Meridian",
            "symbol": "",
            "type": "fungible",
            "definitionId": "8wRnGribscuUhJJFca4nXwR8iAYtrpUU1JXLqZNXMSou",
            "definitionHex": "75f33110b185717209e3955f228d4a4448801d0ce8ba438a4a268050eeff3f44",
            "holdingId": "DNckKy9rUwohZS51ZYGxpw6Ke2WYusaBwqDecMzqCiqF",
            "metadataId": null,
            "rawSupply": "123456789012345678",
            "displaySupply": "123,456,789.012345678",
            "inferredDecimals": 9,
            "authorityMode": "fixed",
            "authority": null,
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "8wRnGribscuUhJJFca4nXwR8iAYtrpUU1JXLqZNXMSou",
                "hex": "75f33110b185717209e3955f228d4a4448801d0ce8ba438a4a268050eeff3f44",
                "name": "Meridian",
                "type": "fungible",
                "totalSupplyRaw": "123456789012345678",
                "mintAuthority": null,
                "metadataId": null
            },
            "holding": {
                "id": "DNckKy9rUwohZS51ZYGxpw6Ke2WYusaBwqDecMzqCiqF",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "100000000000000000",
                "displayBalance": "100,000,000"
            },
            "holdings": [
                {
                    "id": "DNckKy9rUwohZS51ZYGxpw6Ke2WYusaBwqDecMzqCiqF",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "100000000000000000",
                    "displayBalance": "100,000,000"
                },
                {
                    "id": "4TkHNLiC6WQeFkeTmh6bAcLzYYhqbeJyRci92bXAcF8D",
                    "wallet": "mixed-assets",
                    "role": "recipient",
                    "rawBalance": "23456789012345678",
                    "displayBalance": "23,456,789.012345678"
                }
            ],
            "metadata": null
        },
        {
            "id": "HwzCapv2fVYKhrdK68Q7QwdhfNkJDXrCGxqkW8e9UjDG",
            "name": "Quartz",
            "symbol": "",
            "type": "fungible",
            "definitionId": "HwzCapv2fVYKhrdK68Q7QwdhfNkJDXrCGxqkW8e9UjDG",
            "definitionHex": "fbd107ca4bb66bc58f59ac2d32a759be3ee0fb453f8fecd1991c11837d9660c7",
            "holdingId": "AtBFdo6vXRoDHwadHPHvMk84bW3fLtjy7YHF5UeS7Trx",
            "metadataId": null,
            "rawSupply": "12345678901234567890123456",
            "displaySupply": "12,345,678.901234567890123456",
            "inferredDecimals": 18,
            "authorityMode": "fixed",
            "authority": null,
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "HwzCapv2fVYKhrdK68Q7QwdhfNkJDXrCGxqkW8e9UjDG",
                "hex": "fbd107ca4bb66bc58f59ac2d32a759be3ee0fb453f8fecd1991c11837d9660c7",
                "name": "Quartz",
                "type": "fungible",
                "totalSupplyRaw": "12345678901234567890123456",
                "mintAuthority": null,
                "metadataId": null
            },
            "holding": {
                "id": "AtBFdo6vXRoDHwadHPHvMk84bW3fLtjy7YHF5UeS7Trx",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "10000000000000000000000000",
                "displayBalance": "10,000,000"
            },
            "holdings": [
                {
                    "id": "AtBFdo6vXRoDHwadHPHvMk84bW3fLtjy7YHF5UeS7Trx",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "10000000000000000000000000",
                    "displayBalance": "10,000,000"
                },
                {
                    "id": "3JqMVkxAnSVuvetAppW5g3jKrFKv5otyhHNUkYw859p2",
                    "wallet": "mixed-assets",
                    "role": "recipient",
                    "rawBalance": "2345678901234567890123456",
                    "displayBalance": "2,345,678.901234567890123456"
                }
            ],
            "metadata": null
        },
        {
            "id": "6juKZuiBbxwymyet38MnsvidbsomcQEYLR8jhaFYp7f4",
            "name": "Devium",
            "symbol": "",
            "type": "fungible",
            "definitionId": "6juKZuiBbxwymyet38MnsvidbsomcQEYLR8jhaFYp7f4",
            "definitionHex": "5547fcb72644d95a385d313b887a96be41ff263bce6150b49fd87276839822bf",
            "holdingId": "63JPusYpdEEtri5ZHcLqfeM9XdnNsbc3pVdwhFnZB6ep",
            "metadataId": null,
            "rawSupply": "1000000750000000000000000000000",
            "displaySupply": "1,000,000,750,000",
            "inferredDecimals": 18,
            "authorityMode": "external",
            "authority": "HLf2CQotnxpjsrG98xrtUb7qoQcXufU6ARtMAdcuP55c",
            "authorityLabel": "Devium Authority",
            "authorityHex": "f2c40429b1e77773dae8c4d498aa0ff02a71d187133dcd87d9403c1de787eaab",
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "6juKZuiBbxwymyet38MnsvidbsomcQEYLR8jhaFYp7f4",
                "hex": "5547fcb72644d95a385d313b887a96be41ff263bce6150b49fd87276839822bf",
                "name": "Devium",
                "type": "fungible",
                "totalSupplyRaw": "1000000750000000000000000000000",
                "mintAuthority": "HLf2CQotnxpjsrG98xrtUb7qoQcXufU6ARtMAdcuP55c",
                "metadataId": null
            },
            "holding": {
                "id": "63JPusYpdEEtri5ZHcLqfeM9XdnNsbc3pVdwhFnZB6ep",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "1000000750000000000000000000000",
                "displayBalance": "1,000,000,750,000"
            },
            "holdings": [
                {
                    "id": "63JPusYpdEEtri5ZHcLqfeM9XdnNsbc3pVdwhFnZB6ep",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "1000000750000000000000000000000",
                    "displayBalance": "1,000,000,750,000"
                }
            ],
            "metadata": null
        },
        {
            "id": "Hqvym6HLGdu3WXZendqNKLL2feRmwFJ46CrVYhVBw1ti",
            "name": "Mint Condition",
            "symbol": "",
            "type": "fungible",
            "definitionId": "Hqvym6HLGdu3WXZendqNKLL2feRmwFJ46CrVYhVBw1ti",
            "definitionHex": "fa43e74a97d79c5f907ff3edabda5ad89bfbd3b0922572e675d4ad3c7b6029c7",
            "holdingId": "HPYP64fjV6fRRk8rDMVFU4JXnh8g9SMpfFLMFVUBirVx",
            "metadataId": null,
            "rawSupply": "164803398874989484820",
            "displaySupply": "164,803,398,874.98948482",
            "inferredDecimals": 9,
            "authorityMode": "renounced",
            "authority": null,
            "initialAuthority": "Hqvym6HLGdu3WXZendqNKLL2feRmwFJ46CrVYhVBw1ti",
            "metadataStandard": null,
            "metadataUri": null,
            "creators": null,
            "description": null,
            "source": "testnet",
            "instruction": "new_fungible_definition",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "Hqvym6HLGdu3WXZendqNKLL2feRmwFJ46CrVYhVBw1ti",
                "hex": "fa43e74a97d79c5f907ff3edabda5ad89bfbd3b0922572e675d4ad3c7b6029c7",
                "name": "Mint Condition",
                "type": "fungible",
                "totalSupplyRaw": "164803398874989484820",
                "mintAuthority": null,
                "metadataId": null
            },
            "holding": {
                "id": "HPYP64fjV6fRRk8rDMVFU4JXnh8g9SMpfFLMFVUBirVx",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "164803398874989484820",
                "displayBalance": "164,803,398,874.98948482"
            },
            "holdings": [
                {
                    "id": "HPYP64fjV6fRRk8rDMVFU4JXnh8g9SMpfFLMFVUBirVx",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "164803398874989484820",
                    "displayBalance": "164,803,398,874.98948482"
                }
            ],
            "metadata": null
        },
        {
            "id": "6L9bqMV8YiRpTNfqrZaZAyAFwqyNKHG2iQehRqANtVxk",
            "name": "Oops! All Metadata",
            "symbol": "",
            "type": "fungible",
            "definitionId": "6L9bqMV8YiRpTNfqrZaZAyAFwqyNKHG2iQehRqANtVxk",
            "definitionHex": "4f3231d8a01e1d79f163bc27fce0c860a4a2f6890280e9d135eafbde0d68ed79",
            "holdingId": "B8juLRsBJLSAaFtFhwx3K3PpcxZ4aKkxnkh6LeFzdjBy",
            "metadataId": "AHUVjXRgedSGexq9ebwHZ2yfoXed2BKfMm2MQAcxE8WZ",
            "rawSupply": "424218967453",
            "displaySupply": "424,218.967453",
            "inferredDecimals": 6,
            "authorityMode": "fixed",
            "authority": null,
            "metadataStandard": "Simple",
            "metadataUri": "data:application/json;base64,eyJuYW1lIjoiT29wcyEgQWxsIE1ldGFkYXRhIiwiZGVzY3JpcHRpb24iOiJBIHRva2VuIHdpdGggbW9yZSBtZXRhZGF0YSB0aGFuIHNlbnNlLiIsIm1vZGUiOiJmaXhlZCIsInN0YW5kYXJkIjoiU2ltcGxlIn0=",
            "creators": "Department of Redundant Metadata",
            "description": "A token with more metadata than sense.",
            "source": "testnet",
            "instruction": "new_definition_with_metadata",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "6L9bqMV8YiRpTNfqrZaZAyAFwqyNKHG2iQehRqANtVxk",
                "hex": "4f3231d8a01e1d79f163bc27fce0c860a4a2f6890280e9d135eafbde0d68ed79",
                "name": "Oops! All Metadata",
                "type": "fungible",
                "totalSupplyRaw": "424218967453",
                "mintAuthority": null,
                "metadataId": "AHUVjXRgedSGexq9ebwHZ2yfoXed2BKfMm2MQAcxE8WZ"
            },
            "holding": {
                "id": "B8juLRsBJLSAaFtFhwx3K3PpcxZ4aKkxnkh6LeFzdjBy",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "424118967453",
                "displayBalance": "424,118.967453"
            },
            "holdings": [
                {
                    "id": "B8juLRsBJLSAaFtFhwx3K3PpcxZ4aKkxnkh6LeFzdjBy",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "424118967453",
                    "displayBalance": "424,118.967453"
                },
                {
                    "id": "Dxd4WrxvZHmbCCVkFB7dzh6UYcKTc6GLegPS1XeAqowr",
                    "wallet": "all-assets",
                    "role": "recipient",
                    "rawBalance": "100000000",
                    "displayBalance": "100"
                }
            ],
            "metadata": {
                "id": "AHUVjXRgedSGexq9ebwHZ2yfoXed2BKfMm2MQAcxE8WZ",
                "standard": "Simple",
                "uri": "data:application/json;base64,eyJuYW1lIjoiT29wcyEgQWxsIE1ldGFkYXRhIiwiZGVzY3JpcHRpb24iOiJBIHRva2VuIHdpdGggbW9yZSBtZXRhZGF0YSB0aGFuIHNlbnNlLiIsIm1vZGUiOiJmaXhlZCIsInN0YW5kYXJkIjoiU2ltcGxlIn0=",
                "creators": "Department of Redundant Metadata",
                "description": "A token with more metadata than sense."
            }
        },
        {
            "id": "Hqfz9dfGeMgWbL9rhUnMPLgdQZUMczjNjUWCTRozkkk7",
            "name": "Pocket Lint Deluxe",
            "symbol": "",
            "type": "fungible",
            "definitionId": "Hqfz9dfGeMgWbL9rhUnMPLgdQZUMczjNjUWCTRozkkk7",
            "definitionHex": "fa32f354408857006f8ea396b0419823bd04436eadb2d273d2618a46b4793ed8",
            "holdingId": "7aDup3sUWkFF2RZnCXyo78BEvaKT3Wf4dWrPTqaMeRXj",
            "metadataId": "GciAureLmwkCKuELgo2YzF1dYhh8Jcu7cMciquuY7no5",
            "rawSupply": "10000000000000000000000000",
            "displaySupply": "10,000,000",
            "inferredDecimals": 18,
            "authorityMode": "external",
            "authority": "2VavnvNLSTNUWhaG4Tkdw86WrRJx5dyoyQYij6KQzz3Y",
            "metadataStandard": "Expanded",
            "metadataUri": "data:application/json;base64,eyJuYW1lIjoiUG9ja2V0IExpbnQgRGVsdXhlIiwiZGVzY3JpcHRpb24iOiJQcmVtaXVtIGxpbnQsIG5vdyB0b2tlbml6ZWQuIiwibW9kZSI6ImV4dGVybmFsLWF1dGhvcml0eSIsImF1dGhvcml0eSI6IjJWYXZudk5MU1ROVVdoYUc0VGtkdzg2V3JSSng1ZHlveVFZaWo2S1F6ejNZIn0=",
            "creators": "2VavnvNLSTNUWhaG4Tkdw86WrRJx5dyoyQYij6KQzz3Y",
            "description": "Premium lint, now tokenized.",
            "source": "testnet",
            "instruction": "new_definition_with_metadata",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "Hqfz9dfGeMgWbL9rhUnMPLgdQZUMczjNjUWCTRozkkk7",
                "hex": "fa32f354408857006f8ea396b0419823bd04436eadb2d273d2618a46b4793ed8",
                "name": "Pocket Lint Deluxe",
                "type": "fungible",
                "totalSupplyRaw": "10000000000000000000000000",
                "mintAuthority": "2VavnvNLSTNUWhaG4Tkdw86WrRJx5dyoyQYij6KQzz3Y",
                "metadataId": "GciAureLmwkCKuELgo2YzF1dYhh8Jcu7cMciquuY7no5"
            },
            "holding": {
                "id": "7aDup3sUWkFF2RZnCXyo78BEvaKT3Wf4dWrPTqaMeRXj",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "10000000000000000000000000",
                "displayBalance": "10,000,000"
            },
            "holdings": [
                {
                    "id": "7aDup3sUWkFF2RZnCXyo78BEvaKT3Wf4dWrPTqaMeRXj",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "10000000000000000000000000",
                    "displayBalance": "10,000,000"
                }
            ],
            "metadata": {
                "id": "GciAureLmwkCKuELgo2YzF1dYhh8Jcu7cMciquuY7no5",
                "standard": "Expanded",
                "uri": "data:application/json;base64,eyJuYW1lIjoiUG9ja2V0IExpbnQgRGVsdXhlIiwiZGVzY3JpcHRpb24iOiJQcmVtaXVtIGxpbnQsIG5vdyB0b2tlbml6ZWQuIiwibW9kZSI6ImV4dGVybmFsLWF1dGhvcml0eSIsImF1dGhvcml0eSI6IjJWYXZudk5MU1ROVVdoYUc0VGtkdzg2V3JSSng1ZHlveVFZaWo2S1F6ejNZIn0=",
                "creators": "2VavnvNLSTNUWhaG4Tkdw86WrRJx5dyoyQYij6KQzz3Y",
                "description": "Premium lint, now tokenized."
            }
        },
        {
            "id": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
            "name": "Sir Mints-a-Lot",
            "symbol": "",
            "type": "fungible",
            "definitionId": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
            "definitionHex": "00fe99e4fbd4c71f92e47c384c6235244c8cce39b6d6367e1e338eca0ffe01cb",
            "holdingId": "B1yzSPqaetRJx19aXUd7xpQje5iYNF2Qwr1SKbFLCf8F",
            "metadataId": "21ByKA4ZCYWm8pfPyhR1Q1tqYT2RRA8JGsDBX1cp24c7",
            "rawSupply": "1000000000000000000",
            "displaySupply": "1,000,000,000",
            "inferredDecimals": 9,
            "authorityMode": "self",
            "authority": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
            "metadataStandard": "Simple",
            "metadataUri": "data:application/json;base64,eyJuYW1lIjoiU2lyIE1pbnRzLWEtTG90IiwiZGVzY3JpcHRpb24iOiJBIHNlbGYtYXV0aG9yaXplZCBtaW50aW5nIGVudGh1c2lhc3QuIiwibW9kZSI6InNlbGYtYXV0aG9yaXR5IiwiYXV0aG9yaXR5IjoiMTR0QXRpeE1CeUZ5SnJjWlZ5V2liaXRuaWpMZ2Q1OVBmeXJqZG5Zem84TGEifQ==",
            "creators": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
            "description": "A self-authorized minting enthusiast.",
            "source": "testnet",
            "instruction": "new_definition_with_metadata",
            "printableCopies": null,
            "masterHolding": null,
            "definition": {
                "id": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
                "hex": "00fe99e4fbd4c71f92e47c384c6235244c8cce39b6d6367e1e338eca0ffe01cb",
                "name": "Sir Mints-a-Lot",
                "type": "fungible",
                "totalSupplyRaw": "1000000000000000000",
                "mintAuthority": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
                "metadataId": "21ByKA4ZCYWm8pfPyhR1Q1tqYT2RRA8JGsDBX1cp24c7"
            },
            "holding": {
                "id": "B1yzSPqaetRJx19aXUd7xpQje5iYNF2Qwr1SKbFLCf8F",
                "wallet": "all-assets",
                "role": "treasury",
                "rawBalance": "1000000000000000000",
                "displayBalance": "1,000,000,000"
            },
            "holdings": [
                {
                    "id": "B1yzSPqaetRJx19aXUd7xpQje5iYNF2Qwr1SKbFLCf8F",
                    "wallet": "all-assets",
                    "role": "treasury",
                    "rawBalance": "1000000000000000000",
                    "displayBalance": "1,000,000,000"
                }
            ],
            "metadata": {
                "id": "21ByKA4ZCYWm8pfPyhR1Q1tqYT2RRA8JGsDBX1cp24c7",
                "standard": "Simple",
                "uri": "data:application/json;base64,eyJuYW1lIjoiU2lyIE1pbnRzLWEtTG90IiwiZGVzY3JpcHRpb24iOiJBIHNlbGYtYXV0aG9yaXplZCBtaW50aW5nIGVudGh1c2lhc3QuIiwibW9kZSI6InNlbGYtYXV0aG9yaXR5IiwiYXV0aG9yaXR5IjoiMTR0QXRpeE1CeUZ5SnJjWlZ5V2liaXRuaWpMZ2Q1OVBmeXJqZG5Zem84TGEifQ==",
                "creators": "14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La",
                "description": "A self-authorized minting enthusiast."
            }
        },
        {
            "id": "3sG2zN3fAXBvs9mdgFHFiJFSFiwiYyaR5HUA3gZZTSXt",
            "name": "Glitchlings",
            "symbol": "",
            "type": "nonFungible",
            "definitionId": "3sG2zN3fAXBvs9mdgFHFiJFSFiwiYyaR5HUA3gZZTSXt",
            "definitionHex": "2a9769e40f12e6d1567bea757974fd9cbb504be1bd8e5a262e6ee5bfaf533d53",
            "holdingId": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv",
            "metadataId": "BxAbq1EK6Hg6y8g296719V9HQJSet4yio5VG9Ub3YHtU",
            "rawSupply": null,
            "displaySupply": null,
            "inferredDecimals": null,
            "authorityMode": "masterHolding",
            "authority": null,
            "metadataStandard": "Expanded",
            "metadataUri": "data:application/json;base64,eyJuYW1lIjoiR2xpdGNobGluZ3MiLCJkZXNjcmlwdGlvbiI6IkxFWiB0ZXN0bmV0IG5vbi1mdW5naWJsZSBjb2xsZWN0aW9uIiwibmV0d29yayI6InRlc3RuZXQiLCJhdXRob3JpdHkiOiI1UVRCRlFwWURaRHJqNTY5V1BCYW1WcWRhdU5rUlJvNERyWUU0WlpabjJQdiJ9",
            "creators": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv",
            "description": "LEZ testnet non-fungible collection",
            "source": "testnet",
            "instruction": "new_definition_with_metadata",
            "printableCopies": 64,
            "masterHolding": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv",
            "definition": {
                "id": "3sG2zN3fAXBvs9mdgFHFiJFSFiwiYyaR5HUA3gZZTSXt",
                "hex": "2a9769e40f12e6d1567bea757974fd9cbb504be1bd8e5a262e6ee5bfaf533d53",
                "name": "Glitchlings",
                "type": "nonFungible",
                "printableSupply": 64,
                "metadataId": "BxAbq1EK6Hg6y8g296719V9HQJSet4yio5VG9Ub3YHtU"
            },
            "holding": {
                "id": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv",
                "wallet": "all-assets",
                "role": "nftMaster",
                "printBalance": 63,
                "printAuthority": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv"
            },
            "holdings": [
                {
                    "id": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv",
                    "wallet": "all-assets",
                    "role": "nftMaster",
                    "printBalance": 63,
                    "printAuthority": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv"
                },
                {
                    "id": "96b6C9RCd5WCa42RgaRMT7ePEJh5GXLLhbBxd15qwA1i",
                    "role": "nftPrintedCopy",
                    "owned": true
                }
            ],
            "printedCopies": [
                {
                    "id": "96b6C9RCd5WCa42RgaRMT7ePEJh5GXLLhbBxd15qwA1i",
                    "owned": true
                }
            ],
            "metadata": {
                "id": "BxAbq1EK6Hg6y8g296719V9HQJSet4yio5VG9Ub3YHtU",
                "standard": "Expanded",
                "uri": "data:application/json;base64,eyJuYW1lIjoiR2xpdGNobGluZ3MiLCJkZXNjcmlwdGlvbiI6IkxFWiB0ZXN0bmV0IG5vbi1mdW5naWJsZSBjb2xsZWN0aW9uIiwibmV0d29yayI6InRlc3RuZXQiLCJhdXRob3JpdHkiOiI1UVRCRlFwWURaRHJqNTY5V1BCYW1WcWRhdU5rUlJvNERyWUU0WlpabjJQdiJ9",
                "creators": "5QTBFQpYDZDrj569WPBamVqdauNkRRo4DrYE4ZZZn2Pv",
                "description": "LEZ testnet non-fungible collection"
            }
        }
    ]

    property var draftDefinitions: []
    property var liveDefinitions: []
    property bool liveDefinitionsLoaded: false
    readonly property var allDefinitions: root.liveDefinitionsLoaded
        ? root.liveDefinitions.concat(root.draftDefinitions)
        : root.fixtureDefinitions.concat(root.draftDefinitions)

    function findDefinition(id) {
        var wantedId = String(id || "")

        for (var index = 0; index < root.allDefinitions.length; ++index) {
            var definition = root.allDefinitions[index]
            if (definition.id === wantedId || definition.definitionId === wantedId)
                return definition
        }

        return null
    }

    function visibleDefinitions(query) {
        var search = String(query || "").trim().toLowerCase()
        if (!search)
            return root.allDefinitions

        var visible = []
        for (var index = 0; index < root.allDefinitions.length; ++index) {
            var definition = root.allDefinitions[index]
            var fields = [definition.name, definition.id, definition.definitionId, definition.type]

            for (var fieldIndex = 0; fieldIndex < fields.length; ++fieldIndex) {
                if (String(fields[fieldIndex] || "").toLowerCase().indexOf(search) !== -1) {
                    visible.push(definition)
                    break
                }
            }
        }

        return visible
    }

    function shortAddress(address) {
        var value = String(address || "")
        return value.length > 13
            ? value.substring(0, 6) + "..." + value.substring(value.length - 4)
            : value
    }

    function addDraft(draft) {
        if (!draft)
            return null

        root.draftDefinitions = root.draftDefinitions.concat([draft])
        return draft
    }

    function setLiveDefinitions(definitions) {
        root.liveDefinitions = definitions || []
        root.liveDefinitionsLoaded = true
    }

    function clearLiveDefinitions() {
        root.liveDefinitions = []
        root.liveDefinitionsLoaded = false
    }
}
