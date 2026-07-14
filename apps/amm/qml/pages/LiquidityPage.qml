import QtQuick
import QtQml
import Logos.Wallet

import "../components/liquidity"
import "../state"

Item {
    id: root

    property var backend: null
    property var runtime: null
    readonly property NewPositionFlow flow: newPositionFlow

    readonly property int pageMargin: 16
    readonly property int preferredWidth: 480

    AmmTheme {
        id: theme
    }

    NewPositionFlow {
        id: newPositionFlow

        backend: root.backend
        runtime: root.runtime
        active: root.visible
    }

    Rectangle {
        anchors.fill: parent
        color: theme.colors.background
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, form.y + form.implicitHeight + root.pageMargin)
        enabled: !confirmationDialog.opened
        flickableDirection: Flickable.VerticalFlick

        NewPositionForm {
            id: form

            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.pageMargin
            width: Math.max(0, Math.min(root.preferredWidth, scroll.width - root.pageMargin * 2))
            theme: theme
            newPositionContext: newPositionFlow.newPositionContext
            flowState: newPositionFlow.viewState

            onQuoteRequested: function(immediate, quoteRequest) {
                newPositionFlow.scheduleQuote(immediate, quoteRequest)
            }

            onConfirmationRequested: function(snapshot) {
                confirmationDialog.openWithSnapshot(snapshot)
            }

            onTokenResolveRequested: function(tokenId) {
                newPositionFlow.resolveToken(tokenId)
            }

            onDraftChanged: newPositionFlow.draftChanged()
            onRefreshRequested: newPositionFlow.refreshContext(true)
        }
    }

    Connections {
        target: newPositionFlow

        function onTokenResolutionFinished(finalResponse) {
            form.finishTokenResolution(finalResponse)
        }

        function onTokenResolutionFailed(code) {
            form.failTokenResolution(code)
        }

        function onPoolActivated(quote) {
            form.acceptPoolActivation(quote)
        }

        function onQuoteRefreshRequested(immediate) {
            form.requestQuote(immediate)
        }

    }

    Component {
        id: liquidityConfirmationSummary

        LiquidityConfirmationSummary { }
    }

    TransactionConfirmationDialog {
        id: confirmationDialog

        title: qsTr("Confirm new position")
        confirmText: qsTr("Submit")
        busy: newPositionFlow.submitting
        summary: liquidityConfirmationSummary

        onConfirmed: function(snapshot) {
            newPositionFlow.confirm(snapshot)
        }
    }
}
