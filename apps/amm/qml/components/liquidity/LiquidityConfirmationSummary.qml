pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import Logos.Controls

ColumnLayout {
    id: root

    property var snapshot: ({})
    property var theme: fallbackTheme

    signal snapshotEdited(var snapshot)

    spacing: 8

    AmmTheme {
        id: fallbackTheme
    }

    function actionText(instruction) {
        if (instruction === "NewDefinition")
            return qsTr("Create pool")
        if (instruction === "AddLiquidity")
            return qsTr("Add liquidity")
        return instruction || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Pair")
        value: root.snapshot.pairText || "-"
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 5
        visible: root.snapshot.instruction === "AddLiquidity"

        Text {
            text: qsTr("LP destination")
            color: "#a1a1aa"
            font.pixelSize: 12
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 4

            AmmSelectionComboBox {
                id: lpDestinationPicker

                objectName: "lpDestinationPicker"
                Layout.fillWidth: true
                theme: root.theme
                model: root.destinationRows()
                enabled: model.length > 1
                currentIndex: root.destinationIndex()
                displayText: currentIndex >= 0
                             ? model[currentIndex].label : qsTr("Select destination")
                labelForOption: function(destination) { return destination.label }
                tooltipText: String(root.snapshot.selectedLpHoldingId || "")
                Accessible.name: qsTr("LP token destination")
                onActivated: function(index) {
                    root.selectDestination(model[index])
                }
            }

            AmmCopyButton {
                objectName: "copyLpDestinationButton"
                Layout.preferredWidth: visible ? implicitWidth : 0
                Layout.preferredHeight: implicitHeight
                theme: root.theme
                value: String(root.snapshot.selectedLpHoldingId || "")
            }
        }
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Action")
        value: root.actionText(root.snapshot.instruction)
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Fee")
        value: root.snapshot.feeText || "-"
    }

    SummaryRow {
        objectName: "confirmationDeposit"
        Layout.fillWidth: true
        label: root.snapshot.depositLabel || qsTr("Deposit")
        value: qsTr("%1 + %2")
            .arg(root.snapshot.depositAText || "-")
            .arg(root.snapshot.depositBText || "-")
        valueWrapAnywhere: true
    }

    SummaryRow {
        objectName: "confirmationInitialPrice"
        Layout.fillWidth: true
        visible: root.isMissingPool() && String(root.snapshot.initialPriceText || "").length > 0
        label: qsTr("Initial price")
        value: root.snapshot.initialPriceText || "-"
        valueWrapAnywhere: true
    }

    SummaryRow {
        objectName: "confirmationInversePrice"
        Layout.fillWidth: true
        visible: root.isMissingPool() && String(root.snapshot.inverseInitialPriceText || "").length > 0
        label: qsTr("Inverse price")
        value: root.snapshot.inverseInitialPriceText || "-"
        valueWrapAnywhere: true
    }

    SummaryRow {
        objectName: "confirmationDepositMultiplier"
        Layout.fillWidth: true
        visible: root.isMissingPool() && String(root.snapshot.depositMultiplierText || "").length > 0
        label: qsTr("Deposit multiplier")
        value: root.snapshot.depositMultiplierText || "-"
    }

    SummaryRow {
        objectName: "confirmationDepositScale"
        Layout.fillWidth: true
        visible: root.isMissingPool() && String(root.snapshot.depositScaleText || "").length > 0
        label: qsTr("Deposit scale")
        value: root.snapshot.depositScaleText || "-"
    }

    SummaryRow {
        objectName: "confirmationExpectedLp"
        Layout.fillWidth: true
        label: qsTr("Expected LP")
        value: root.snapshot.expectedLpText || "-"
    }

    SummaryRow {
        objectName: "confirmationLpGuard"
        Layout.fillWidth: true
        label: root.snapshot.lpGuardLabel || qsTr("Minimum LP")
        value: root.snapshot.lpGuardText || "-"
    }

    CopyableAddressRow {
        objectName: "poolAddressRow"
        Layout.fillWidth: true
        visible: String(root.snapshot.poolId || "").length > 0
        theme: root.theme
        label: qsTr("Pool")
        address: String(root.snapshot.poolId || "")
    }

    LogosButton {
        id: accountPlanButton

        objectName: "confirmationAccountPlan"
        Layout.alignment: Qt.AlignLeft
        visible: root.accountPlan().length > 0
        text: qsTr("Account plan (%1)").arg(root.accountPlan().length)
        property bool expanded: false
        implicitWidth: 150
        implicitHeight: 36
        radius: 6
        onClicked: expanded = !expanded
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 5
        visible: accountPlanButton.visible && accountPlanButton.expanded

        Repeater {
            model: root.accountPlan()

            CopyableAddressRow {
                required property var modelData

                objectName: "accountPlanAddressRow" + String(modelData.order || 0)
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("%1. %2 · %3")
                    .arg(Number(modelData.order || 0) + 1)
                    .arg(String(modelData.role || "-"))
                    .arg(String(modelData.action || "-"))
                address: String(modelData.accountId || "")
                fallbackText: qsTr("Assigned by wallet")
            }
        }
    }

    function isMissingPool() {
        return root.snapshot.poolStatus === "missing_pool"
    }

    function accountPlan() {
        return root.snapshot.accountPreview || []
    }

    function destinationRows() {
        var rows = []
        var options = root.snapshot.lpHoldingOptions || []
        for (var i = 0; i < options.length; ++i) {
            var id = String(options[i].holdingId || "")
            rows.push({
                "holdingId": id,
                "createFresh": false,
                "label": qsTr("%1 · balance %2")
                    .arg(root.shortId(id))
                    .arg(String(options[i].balanceRaw || "0"))
            })
        }
        rows.push({
            "holdingId": "",
            "createFresh": true,
            "label": qsTr("Create new LP holding")
        })
        return rows
    }

    function destinationIndex() {
        var rows = root.destinationRows()
        for (var i = 0; i < rows.length; ++i) {
            if (rows[i].createFresh === (root.snapshot.createFreshLp === true)
                    && (rows[i].createFresh
                        || rows[i].holdingId === root.snapshot.selectedLpHoldingId)) {
                return i
            }
        }
        return -1
    }

    function selectDestination(destination) {
        var next = JSON.parse(JSON.stringify(root.snapshot || ({})))
        next.request = next.request || ({})
        delete next.request.lpHoldingId
        next.request.createFreshLp = destination.createFresh === true
        if (!destination.createFresh)
            next.request.lpHoldingId = destination.holdingId
        next.selectedLpHoldingId = destination.holdingId
        next.createFreshLp = destination.createFresh === true
        next.lpDestinationRequired = false
        next.quoteReady = false
        root.snapshotEdited(next)
    }

    function shortId(value) {
        return value.length > 14 ? value.slice(0, 7) + "…" + value.slice(-5) : value
    }
}
