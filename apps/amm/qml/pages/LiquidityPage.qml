import QtQuick
import QtQml

import "../components/liquidity"

Item {
    id: root

    property var backend: null
    property var newPositionContext: ({
        "schema": "new-position.v1",
        "status": "loading",
        "tokens": [],
        "feeTiers": []
    })
    property var newPositionQuote: ({})
    property var resolvedTokenIds: []
    property int quoteSerial: 0
    property bool componentReady: false
    property bool contextLoading: false
    property bool quoteLoading: false
    property bool quoteStale: true
    property bool submitting: false
    property bool refreshingAfterSuccess: false
    property string transactionId: ""
    property string refreshWarning: ""
    property string lastContextJson: ""

    readonly property int pageMargin: 16
    readonly property int preferredWidth: 800

    Component.onCompleted: {
        root.componentReady = true
        root.refreshNewPositionContext()
    }

    onBackendChanged: {
        if (root.componentReady)
            root.refreshNewPositionContext()
    }

    Connections {
        target: root.backend
        ignoreUnknownSignals: true

        function onNewPositionContextChanged(value) {
            root.adoptContext(value)
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "#151515"
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, form.y + form.implicitHeight + root.pageMargin)
        enabled: !confirmationDialog.open
        flickableDirection: Flickable.VerticalFlick

        NewPositionForm {
            id: form

            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.pageMargin
            width: Math.max(0, Math.min(root.preferredWidth, scroll.width - root.pageMargin * 2))

            newPositionContext: root.newPositionContext
            quotePayload: root.newPositionQuote
            contextLoading: root.contextLoading
            quoteLoading: root.quoteLoading
            quoteStale: root.quoteStale
            submitting: root.submitting
            transactionId: root.transactionId
            refreshWarning: root.refreshWarning

            onQuoteRequested: function(immediate) {
                root.scheduleQuote(immediate)
            }

            onConfirmationRequested: function(snapshot) {
                confirmationDialog.openWithSnapshot(snapshot)
            }

            onTokenResolveRequested: function(tokenId) {
                root.resolveToken(tokenId)
            }

            onDraftChanged: {
                root.transactionId = ""
                root.refreshWarning = ""
            }

            onRefreshRequested: root.refreshNewPositionContext()
        }
    }

    Timer {
        id: quoteDebounce
        interval: 250
        repeat: false
        onTriggered: root.requestQuoteNow(root.quoteSerial)
    }

    NewPositionConfirmationDialog {
        id: confirmationDialog

        anchors.fill: parent
        busy: root.submitting

        onConfirmed: function(snapshot) {
            root.confirmNewPosition(snapshot)
        }
    }

    function contextHints() {
        var recent = []
        if (form.selectedTokenAId.length > 0)
            recent.push(form.selectedTokenAId)
        if (form.selectedTokenBId.length > 0
                && form.selectedTokenBId !== form.selectedTokenAId) {
            recent.push(form.selectedTokenBId)
        }
        return {
            "recentTokenIds": recent,
            "resolvedTokenIds": root.resolvedTokenIds
        }
    }

    function refreshNewPositionContext() {
        root.contextLoading = true
        if (!root.backend || typeof logos === "undefined") {
            root.contextLoading = false
            root.newPositionContext = root.contextError("wallet_unavailable")
            return
        }

        logos.watch(root.backend.refreshNewPositionContext(root.contextHints()),
            function(context) {
                root.adoptContext(context)
                if (root.refreshingAfterSuccess) {
                    root.refreshingAfterSuccess = false
                    root.refreshWarning = context.status === "ready" || context.status === "no_wallet"
                                          ? ""
                                          : qsTr("Balances could not be refreshed.")
                }
            },
            function(error) {
                root.contextLoading = false
                if (root.refreshingAfterSuccess) {
                    root.refreshingAfterSuccess = false
                    root.refreshWarning = qsTr("Balances could not be refreshed.")
                } else {
                    root.newPositionContext = root.contextError("backend_error")
                }
            })
    }

    function adoptContext(context) {
        if (!context || context.schema !== "new-position.v1")
            context = root.contextError("unsupported_schema")
        var serialized = JSON.stringify(context)
        root.contextLoading = false
        if (serialized === root.lastContextJson)
            return
        root.lastContextJson = serialized
        root.newPositionContext = context
        root.quoteStale = true
        ++root.quoteSerial
    }

    function resolveToken(tokenId) {
        var value = String(tokenId || "").trim()
        if (value.length === 0)
            return
        if (root.resolvedTokenIds.indexOf(value) < 0) {
            var next = root.resolvedTokenIds.slice(0)
            next.push(value)
            root.resolvedTokenIds = next
        }
        root.refreshNewPositionContext()
    }

    function scheduleQuote(immediate) {
        ++root.quoteSerial
        root.quoteStale = true
        root.quoteLoading = true
        quoteDebounce.stop()
        if (immediate)
            root.requestQuoteNow(root.quoteSerial)
        else
            quoteDebounce.restart()
    }

    function requestQuoteNow(serial) {
        if (serial !== root.quoteSerial)
            return
        var built = form.buildQuoteRequest()
        if (!built.ok) {
            root.quoteLoading = false
            return
        }
        if (!root.backend || typeof logos === "undefined") {
            root.quoteLoading = false
            root.newPositionQuote = root.quoteError("wallet_unavailable")
            return
        }

        logos.watch(root.backend.quoteNewPosition(built.request),
            function(quote) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = false
                if (!quote || quote.schema !== "new-position.v1")
                    root.newPositionQuote = root.quoteError("unsupported_schema")
                else
                    root.newPositionQuote = quote
            },
            function(error) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = true
                form.submitError = qsTr("Quote request failed. Refresh and retry.")
            })
    }

    function confirmNewPosition(snapshot) {
        if (root.submitting)
            return
        root.submitting = true
        form.submitError = ""

        if (!root.backend || typeof logos === "undefined") {
            root.finishSubmitFailure(root.quoteError("wallet_unavailable"))
            return
        }

        logos.watch(root.backend.submitNewPosition(snapshot.request, snapshot.quoteHash),
            function(result) {
                if (result && result.schema === "new-position.v1"
                        && result.status === "submitted"
                        && /^[0-9a-f]{64}$/.test(String(result.transactionId || ""))) {
                    root.submitting = false
                    confirmationDialog.closeAfterSuccess()
                    root.transactionId = result.transactionId
                    root.refreshWarning = ""
                    form.resetAfterSubmit()
                    root.newPositionQuote = ({})
                    root.quoteStale = true
                    root.refreshingAfterSuccess = true
                    root.refreshNewPositionContext()
                    return
                }
                root.finishSubmitFailure(result || root.quoteError("wallet_submission_failed"))
            },
            function(error) {
                root.finishSubmitFailure(root.quoteError("wallet_submission_failed"))
            })
    }

    function finishSubmitFailure(result) {
        root.submitting = false
        confirmationDialog.closeAfterFailure()
        if (result && result.quote && result.quote.schema === "new-position.v1")
            root.newPositionQuote = result.quote
        var code = result && result.code ? result.code : "wallet_submission_failed"
        form.submitError = form.issueText(code)
        root.scheduleQuote(true)
    }

    function contextError(code) {
        return {
            "schema": "new-position.v1",
            "status": "error",
            "code": code,
            "tokens": [],
            "feeTiers": []
        }
    }

    function quoteError(code) {
        return {
            "schema": "new-position.v1",
            "status": "error",
            "canSubmit": false,
            "code": code,
            "poolStatus": "unavailable_pool",
            "errors": [{
                "code": code,
                "blockingFields": [],
                "details": ({})
            }],
            "warnings": [],
            "accountPreview": []
        }
    }
}
