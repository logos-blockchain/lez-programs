import QtQuick 2.15
import QtQml 2.15
import "../components/shared"
import "../components/liquidity"

Item {
    id: root

    property var backend: null
    property var newPositionContext: ({})
    property var newPositionQuote: root.errorQuote(qsTr("Wallet backend is unavailable."), root.currentRequest())
    property int quoteSerial: 0
    property bool formReady: false
    property bool contextLoading: false
    property bool quoteLoading: false
    property bool submitting: false

    readonly property int pageMargin: 16
    readonly property int preferredCardWidth: 960
    readonly property int pageCardY: newPositionForm.implicitHeight + root.pageMargin * 2 <= scroll.height ? Math.max(root.pageMargin, Math.round((scroll.height - newPositionForm.implicitHeight) / 4)) : root.pageMargin

    width: parent ? parent.width : implicitWidth
    height: parent ? parent.height : implicitHeight
    implicitWidth: root.preferredCardWidth + root.pageMargin * 2
    implicitHeight: newPositionForm.implicitHeight + root.pageMargin * 2

    Component.onCompleted: root.refreshNewPositionContext()
    onBackendChanged: root.refreshNewPositionContext()

    Connections {
        target: root.backend
        ignoreUnknownSignals: true

        function onNewPositionContextChanged(value) {
            root.newPositionContext = value;
            root.requestQuote(root.currentRequest());
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
        contentHeight: Math.max(height, newPositionForm.y + newPositionForm.implicitHeight + root.pageMargin)
        contentWidth: width
        enabled: !confirmationDialog.open
        flickableDirection: Flickable.VerticalFlick

        NewPositionForm {
            id: newPositionForm

            newPositionContext: root.newPositionContext
            quotePayload: root.newPositionQuote
            contextLoading: root.contextLoading
            quoteLoading: root.quoteLoading
            submitting: root.submitting
            width: Math.max(0, Math.min(scroll.width - root.pageMargin * 2, root.preferredCardWidth))
            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.pageCardY

            Component.onCompleted: {
                root.formReady = true;
                root.refreshNewPositionContext();
            }

            onQuoteRequested: function (request) {
                root.requestQuote(request);
            }

            onConfirmationRequested: function (snapshot) {
                confirmationDialog.openWithSnapshot(snapshot);
            }
        }
    }

    SuccessToast {
        id: successToast

        width: Math.max(0, Math.min(380, root.width - 32))
        z: 30

        anchors {
            bottom: parent.bottom
            bottomMargin: 18
            horizontalCenter: parent.horizontalCenter
        }
    }

    NewPositionConfirmationDialog {
        id: confirmationDialog

        anchors.fill: parent
        busy: root.submitting

        onConfirmed: function (snapshot) {
            root.confirmNewPosition(snapshot);
        }
    }

    function confirmNewPosition(snapshot) {
        if (root.submitting)
            return;

        root.submitting = true;
        confirmationDialog.errorText = "";
        newPositionForm.setSubmitError("");

        if (!root.backend || typeof logos === "undefined") {
            root.submitting = false;
            root.showSubmitError(qsTr("Wallet backend is unavailable."));
            return;
        }

        logos.watch(root.backend.submitNewPosition(snapshot.request, snapshot.quoteHash),
            function(result) {
                root.submitting = false;
                if (result.status !== "ok") {
                    root.showSubmitError(result.error);
                    return;
                }

                confirmationDialog.closeAfterSuccess();
                newPositionForm.resetAfterSubmit();
                successToast.show(result.message, result.detail);
            },
            function(error) {
                root.submitting = false;
                root.showSubmitError(qsTr("Error submitting position: %1").arg(error));
            });
    }

    function refreshNewPositionContext() {
        root.contextLoading = true;

        if (!root.backend || typeof logos === "undefined") {
            root.contextLoading = false;
            root.newPositionContext = {
                "activeAccountDisplay": qsTr("Not connected"),
                "holdings": [],
                "feeTiers": []
            };
            root.newPositionQuote = root.errorQuote(qsTr("Wallet backend is unavailable."), root.currentRequest());
            return;
        }

        logos.watch(root.backend.refreshNewPositionContext(),
            function(context) {
                root.contextLoading = false;
                root.newPositionContext = context;
                root.requestQuote(root.currentRequest());
            },
            function(error) {
                root.contextLoading = false;
                root.newPositionQuote = root.errorQuote(qsTr("Error loading position context: %1").arg(error), root.currentRequest());
            });
    }

    function requestQuote(request) {
        root.quoteLoading = true;

        if (!root.backend || typeof logos === "undefined") {
            root.quoteLoading = false;
            root.newPositionQuote = root.errorQuote(qsTr("Wallet backend is unavailable."), request);
            return;
        }

        const serial = ++root.quoteSerial;
        logos.watch(root.backend.quoteNewPosition(request),
            function(quote) {
                if (serial === root.quoteSerial) {
                    root.quoteLoading = false;
                    root.newPositionQuote = quote;
                }
            },
            function(error) {
                if (serial === root.quoteSerial) {
                    root.quoteLoading = false;
                    root.newPositionQuote = root.errorQuote(qsTr("Error loading quote: %1").arg(error), request);
                }
            });
    }

    function showSubmitError(message) {
        const text = message && message.length > 0 ? message : qsTr("Position submission failed.");
        confirmationDialog.errorText = text;
        newPositionForm.setSubmitError(text);
    }

    function amountValue(symbol) {
        return {
            "value": 0,
            "input": "0",
            "display": qsTr("0 %1").arg(symbol),
            "symbol": symbol
        };
    }

    function currentRequest() {
        if (root.formReady)
            return newPositionForm.buildRequest();

        return {
            "amountA": "",
            "amountB": "",
            "depositScale": 1,
            "editedSide": "A",
            "feeBps": 30,
            "initialPrice": "1",
            "slippageBps": 50,
            "tokenA": "",
            "tokenB": ""
        };
    }

    function errorQuote(message, request) {
        const tokenA = request ? request.tokenA : "";
        const tokenB = request ? request.tokenB : "";
        return {
            "status": "error",
            "error": message,
            "poolStatus": "unavailable_pool",
            "statusLabel": qsTr("Unavailable"),
            "statusDetail": message,
            "instruction": "",
            "storedFeeBps": 0,
            "feeBps": request ? request.feeBps : 0,
            "feeLabel": "",
            "quoteHash": "",
            "pool": {
                "id": "",
                "priceText": "",
                "reserveText": ""
            },
            "deposit": {
                "maxA": root.amountValue(tokenA),
                "maxB": root.amountValue(tokenB),
                "actualA": root.amountValue(tokenA),
                "actualB": root.amountValue(tokenB)
            },
            "lp": {
                "expected": root.amountValue("LP"),
                "minimum": root.amountValue("LP"),
                "locked": root.amountValue("LP")
            },
            "position": {
                "userLp": "0 LP",
                "share": "-",
                "ownedA": qsTr("0 %1").arg(tokenA),
                "ownedB": qsTr("0 %1").arg(tokenB)
            },
            "accountChanges": []
        };
    }
}
