import QtQuick
import QtTest
import "../../qml/state" as State

Item {
    id: scene
    width: 800
    height: 600

    Component {
        id: backendComponent
        QtObject {
            property bool walletStateReady: true
            property var poolReply: ({})
            property int createQuotes: 0
            property int addQuotes: 0
            function resolvePoolAccount(a, b) { return poolReply }
            function createPoolQuote(request) {
                ++createQuotes
                return { status: "error", error: "quote_marker" }
            }
            function addLiquidityQuote(request) {
                ++addQuotes
                return { status: "error", error: "quote_marker" }
            }
            function createPool(request) {
                return { status: "error", error: "already_exists" }
            }
        }
    }

    Component {
        id: runtimeComponent
        QtObject {
            function watch(request, success, failure) { success(request) }
        }
    }

    Component {
        id: flowComponent
        State.NewPositionFlow { active: true }
    }

    TestCase {
        name: "NewPositionFlowPoolErrors"
        when: windowShown

        function test_poolResolution_data() {
            return [
                { tag: "read-failure", status: "error", error: "pool_read_failed",
                  creates: 0, adds: 0, exists: undefined, quoteError: "pool_read_failed" },
                { tag: "occupied-address", status: "error", error: "bad_config",
                  creates: 0, adds: 0, exists: undefined, quoteError: "bad_config" },
                { tag: "missing", status: "error", error: "no_pool",
                  creates: 1, adds: 0, exists: false, quoteError: "quote_marker" },
                { tag: "existing", status: "ok", error: "",
                  creates: 0, adds: 1, exists: true, quoteError: "quote_marker" }
            ]
        }

        function test_poolResolution(data) {
            const backend = createTemporaryObject(backendComponent, scene,
                { poolReply: { status: data.status, error: data.error } })
            const runtime = createTemporaryObject(runtimeComponent, scene)
            const flow = createTemporaryObject(flowComponent, scene,
                { backend: backend, runtime: runtime })
            verify(!!flow, "Component exists")
            flow.scheduleQuote(true, { ok: true, request: { tokenAId: "a", tokenBId: "b" } })
            compare(backend.createQuotes, data.creates)
            compare(backend.addQuotes, data.adds)
            compare(flow.poolExists, data.exists)
            compare(flow.viewState.quote.code, data.quoteError)
            compare(flow.quoteLoading, false)
        }

        function test_occupiedAddressOnSubmission() {
            const backend = createTemporaryObject(backendComponent, scene)
            const runtime = createTemporaryObject(runtimeComponent, scene)
            const flow = createTemporaryObject(flowComponent, scene,
                { backend: backend, runtime: runtime })
            verify(!!flow, "Component exists")
            flow.submitting = true
            flow.submitCreatePool({ request: { tokenAId: "a", tokenBId: "b",
                                               amountA: "1", amountB: "2" },
                                    holdingAId: "a-holding", holdingBId: "b-holding" }, "lp")
            compare(flow.submitting, false)
            compare(flow.viewState.errorCode, "already_exists")
            compare(flow.transactionId, "")
        }
    }
}
