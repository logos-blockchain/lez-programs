import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml

import Logos.Controls
import Logos.Icons
import Logos.Wallet

import "../components/liquidity"
import "../state"

Item {
    id: root

    objectName: "liquidityPage"

    property var backend: null
    property var runtime: null
    readonly property NewPositionFlow flow: newPositionFlow

    // Wallet token holdings (backend.tokenHoldings()) feeding the create-pool
    // account selectors; refetched when the wallet opens.
    property var holdings: []

    // The AMM's supported fee tiers (backend.feeTiers()) feeding the fee selector.
    // Program-derived and wallet-independent, so it's fetched once when the backend
    // becomes available.
    property var feeTiers: []

    // The liquidity token selector rows (backend.resolveTokens()): the app-owned union of
    // configured tokens and persisted-custom tokens. Refetched when the wallet opens/closes
    // (holdingId/balance change) and after a custom token is added.
    property var resolvedTokens: []
    property bool tokensLoading: false
    property int tokensGeneration: 0

    // A pair handed over from the pool detail view. Held until resolveTokens()
    // has answered, since the handoff can arrive before the selector's rows do.
    property var pendingPair: null

    // Clears the form back to a first-visit state. Called by Main.qml whenever the
    // page is navigated to, so a previous visit's pair and amounts don't linger.
    // A pair handed over by selectPair() is applied after this, not before.
    function resetForm() {
        root.pendingPair = null
        form.resetAll()
    }

    // Preselects a pool's pair on the new-position form. Called by Main.qml when
    // the pool detail view's "Add liquidity" button is pressed.
    function selectPair(pool) {
        if (!pool)
            return
        root.pendingPair = pool
        root.applyPendingPair()
    }

    // Only an id the selector actually offers can be preselected: the form drops
    // any selection that isn't in resolveTokens(), and the pool config could
    // carry a different id encoding than the resolved rows.
    function selectableTokenId(definitionId) {
        if (definitionId.length === 0)
            return ""
        for (var i = 0; i < root.resolvedTokens.length; ++i) {
            if (String(root.resolvedTokens[i].definitionId) === definitionId)
                return definitionId
        }
        return ""
    }

    function applyPendingPair() {
        // Guard on the list itself, not on tokensLoading: refreshTokens() assigns
        // resolvedTokens before it clears the flag, so this runs from
        // onResolvedTokensChanged while the load is still nominally in progress.
        if (!root.pendingPair || root.resolvedTokens.length === 0)
            return
        var pair = root.pendingPair
        // One attempt per handoff — see SwapPage.applyPendingPair().
        root.pendingPair = null
        var idA = root.selectableTokenId(String(pair.tokenADefinitionId || ""))
        var idB = root.selectableTokenId(String(pair.tokenBDefinitionId || ""))
        if (idA.length > 0)
            form.selectToken("A", idA)
        if (idB.length > 0)
            form.selectToken("B", idB)
    }

    onResolvedTokensChanged: root.applyPendingPair()

    function refreshHoldings() {
        if (!root.backend || root.runtime === null)
            return
        root.runtime.watch(root.backend.tokenHoldings(),
            function(list) { root.holdings = list },
            function(err) { console.warn("tokenHoldings error:", err) })
    }

    function refreshFeeTiers() {
        if (!root.backend || root.runtime === null || root.feeTiers.length > 0)
            return
        root.runtime.watch(root.backend.feeTiers(),
            function(list) { root.feeTiers = list },
            function(err) { console.warn("feeTiers error:", err) })
    }

    function refreshTokens() {
        if (!root.backend || root.runtime === null)
            return
        // Tag each request; a wallet toggle can start overlapping resolveTokens calls whose
        // replies arrive out of order — drop any superseded callback (mirrors SwapPage's
        // holdings-generation guard).
        const generation = ++root.tokensGeneration
        root.tokensLoading = true
        root.runtime.watch(root.backend.resolveTokens(),
            function(list) {
                if (generation !== root.tokensGeneration)
                    return
                root.resolvedTokens = list
                root.tokensLoading = false
            },
            function(err) {
                if (generation !== root.tokensGeneration)
                    return
                root.tokensLoading = false
                console.warn("resolveTokens error:", err)
            })
    }

    // Validates + persists a user-pasted custom token id, then refreshes the list and hands the
    // resolved row back to the form to complete selection (or reports the failure).
    function addCustomToken(tokenId) {
        if (!root.backend || root.runtime === null) {
            form.failTokenResolution("backend_error")
            return
        }
        root.runtime.watch(root.backend.addCustomToken(tokenId),
            function(result) {
                if (result && result.ok === true) {
                    root.refreshTokens()
                    form.finishTokenResolution(result.token)
                } else {
                    form.failTokenResolution(result && result.error ? result.error : "unresolved")
                }
            },
            function(err) {
                console.warn("addCustomToken error:", err)
                form.failTokenResolution("backend_error")
            })
    }

onBackendChanged: { root.refreshHoldings(); root.refreshFeeTiers(); root.refreshTokens() }
    onRuntimeChanged: { root.refreshHoldings(); root.refreshFeeTiers(); root.refreshTokens() }
    Component.onCompleted: { root.refreshHoldings(); root.refreshFeeTiers(); root.refreshTokens() }

    Connections {
        target: root.backend
        function onIsWalletOpenChanged() { root.refreshHoldings(); root.refreshTokens() }
    }

    readonly property int pageMargin: width < 640 ? 16 : 24
    readonly property int contentMaxWidth: 1200
    readonly property bool wideLayout: width >= 760
    readonly property int stepRailWidth: Math.max(210, Math.min(330,
                                                                  (width - pageMargin * 2) * 0.30))

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
        contentHeight: Math.max(height, pageLayout.y + pageLayout.implicitHeight + root.pageMargin)
        enabled: !confirmationDialog.opened
        flickableDirection: Flickable.VerticalFlick
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
        }

        ColumnLayout {
            id: pageLayout

            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.wideLayout ? 32 : 16
            width: Math.max(0, Math.min(root.contentMaxWidth,
                                        scroll.width - root.pageMargin * 2))
            spacing: 24

            RowLayout {
                Layout.fillWidth: true
                spacing: 16

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 4

                    Text {
                        text: qsTr("New position")
                        color: theme.colors.textPrimary
                        font.pixelSize: 30
                        font.weight: Font.Bold
                        font.letterSpacing: 0
                    }

                    Text {
                        Layout.fillWidth: true
                        text: form.contextStatusText()
                        color: theme.colors.textSecondary
                        font.pixelSize: 12
                        elide: Text.ElideRight
                    }
                }

                LogosIconButton {
                    objectName: "refreshPositionButton"
                    iconSource: LogosIcons.refresh
                    iconColor: theme.colors.textSecondary
                    iconSize: 18
                    Layout.preferredWidth: 40
                    Layout.preferredHeight: 40
                    enabled: !root.tokensLoading && !newPositionFlow.submitting
                    Accessible.name: qsTr("Refresh position data")
                    ToolTip.visible: hovered
                    ToolTip.text: Accessible.name
                    onClicked: { root.refreshTokens(); root.refreshHoldings() }
                }
            }

            Rectangle {
                id: compactSteps

                objectName: "compactPositionSteps"
                Layout.fillWidth: true
                implicitHeight: compactStepRow.implicitHeight + 32
                visible: !root.wideLayout
                radius: 16
                color: theme.colors.cardBg
                border.color: theme.colors.border
                border.width: 1

                RowLayout {
                    id: compactStepRow

                    anchors.fill: parent
                    anchors.margins: 16
                    spacing: 12

                    StepMarker {
                        Layout.fillWidth: true
                        colors: theme.colors
                        stepNumber: 1
                        label: qsTr("Select pair and fees")
                        active: !form.hasPair
                        complete: form.hasPair
                    }

                    Rectangle {
                        Layout.preferredWidth: 20
                        Layout.preferredHeight: 1
                        color: form.hasPair ? theme.colors.ctaBg : theme.colors.divider
                    }

                    StepMarker {
                        Layout.fillWidth: true
                        colors: theme.colors
                        stepNumber: 2
                        label: form.missingPool
                               ? qsTr("Set price and deposit")
                               : qsTr("Enter deposit amounts")
                        active: form.hasPair
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                spacing: root.wideLayout ? 40 : 0

                Rectangle {
                    id: positionStepRail

                    objectName: "positionStepRail"
                    Layout.preferredWidth: root.stepRailWidth
                    Layout.alignment: Qt.AlignTop
                    implicitHeight: verticalSteps.implicitHeight + 40
                    visible: root.wideLayout
                    radius: 16
                    color: theme.colors.cardBg
                    border.color: theme.colors.border
                    border.width: 1

                    ColumnLayout {
                        id: verticalSteps

                        anchors.fill: parent
                        anchors.margins: 20
                        spacing: 0

                        StepMarker {
                            Layout.fillWidth: true
                            colors: theme.colors
                            stepNumber: 1
                            label: qsTr("Select token pair and fees")
                            active: !form.hasPair
                            complete: form.hasPair
                        }

                        Rectangle {
                            Layout.leftMargin: 17
                            Layout.preferredWidth: 1
                            Layout.preferredHeight: 28
                            color: form.hasPair ? theme.colors.ctaBg : theme.colors.divider
                        }

                        StepMarker {
                            Layout.fillWidth: true
                            colors: theme.colors
                            stepNumber: 2
                            label: form.missingPool
                                   ? qsTr("Set price and deposit amounts")
                                   : qsTr("Enter deposit amounts")
                            active: form.hasPair
                        }
                    }
                }

                NewPositionForm {
                    id: form

                    objectName: "newPositionForm"
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    theme: theme
                    headingText: form.hasPair ? qsTr("Deposit tokens") : qsTr("Select pair")
                    headingDetail: form.hasPair
                                   ? qsTr("Specify the token amounts for your liquidity contribution.")
                                   : qsTr("Choose two tokens and a fee tier for this position.")
                    showRefreshAction: false
                    holdings: root.holdings
                    feeTiers: root.feeTiers
                    tokens: root.resolvedTokens
                    loadingTokens: root.tokensLoading
                    walletReady: newPositionFlow.walletStateReady
                    flowState: newPositionFlow.viewState

                    onQuoteRequested: function(immediate, quoteRequest) {
                        newPositionFlow.scheduleQuote(immediate, quoteRequest)
                    }

                    onConfirmationRequested: function(snapshot) {
                        confirmationDialog.openWithSnapshot(snapshot)
                    }

                    onTokenResolveRequested: function(tokenId) {
                        root.addCustomToken(tokenId)
                    }

                    onDraftChanged: newPositionFlow.draftChanged()
                    onPairReset: newPositionFlow.resetPoolExistence()
                    onRefreshRequested: { root.refreshTokens(); root.refreshHoldings() }
                }
            }
        }
    }

    Connections {
        target: newPositionFlow

        // Token resolution now goes app-side (addCustomToken → form callbacks); the flow only
        // signals when a pool-existence change should re-request the quote.
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

        objectName: "liquidityConfirmDialog"
        title: qsTr("Confirm new position")
        confirmText: qsTr("Submit")
        busy: newPositionFlow.submitting
        summary: liquidityConfirmationSummary

        onConfirmed: function(snapshot) {
            newPositionFlow.confirm(snapshot)
        }
    }

    component StepMarker: RowLayout {
        id: marker

        required property var colors
        required property int stepNumber
        required property string label
        property bool active: false
        property bool complete: false

        spacing: 12

        Rectangle {
            Layout.preferredWidth: 36
            Layout.preferredHeight: 36
            radius: 18
            color: marker.active
                   ? marker.colors.ctaBg
                   : marker.complete ? marker.colors.selection : marker.colors.inputBg
            border.color: marker.active || marker.complete
                          ? marker.colors.ctaBg : marker.colors.borderStrong
            border.width: 1
            Accessible.ignored: true

            Text {
                anchors.centerIn: parent
                text: marker.stepNumber
                color: marker.active
                       ? marker.colors.background
                       : marker.complete ? marker.colors.ctaBg : marker.colors.textPlaceholder
                font.pixelSize: 13
                font.weight: Font.DemiBold
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2

            Text {
                Layout.fillWidth: true
                text: qsTr("Step %1").arg(marker.stepNumber)
                color: marker.active || marker.complete
                       ? marker.colors.textSecondary : marker.colors.textPlaceholder
                font.pixelSize: 11
                elide: Text.ElideRight
            }

            Text {
                Layout.fillWidth: true
                text: marker.label
                color: marker.active || marker.complete
                       ? marker.colors.textPrimary : marker.colors.textSecondary
                font.pixelSize: 13
                font.weight: marker.active ? Font.DemiBold : Font.Normal
                wrapMode: Text.Wrap
            }
        }
    }
}
