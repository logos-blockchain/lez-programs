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

    property var backend: null
    property var runtime: null
    readonly property NewPositionFlow flow: newPositionFlow

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
                    enabled: !newPositionFlow.contextLoading && !newPositionFlow.submitting
                    Accessible.name: qsTr("Refresh position data")
                    ToolTip.visible: hovered
                    ToolTip.text: Accessible.name
                    onClicked: newPositionFlow.refreshContext(true)
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
