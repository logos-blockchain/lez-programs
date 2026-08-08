pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic

Item {
    id: root

    enum SelectionMode {
        Input,
        Output
    }

    property var sourceModel: []
    property string accountType: ""
    property string stateField: ""
    property var stateValue: ""
    property int selectionMode: ProgramAccountSelector.Input
    property string selectedAccountId: ""
    property bool createNewSelected: false
    property string createNewText: qsTr("Create new account")
    property string emptyInputText: qsTr("No funds")
    property string placeholderText: qsTr("Select account")
    property string criteriaPendingText: qsTr("Select token first")
    property string accessibleName: qsTr("Program account")
    property color backgroundColor: "#27272a"
    property color hoverColor: "#3f3f46"
    property color textColor: "#f4f4f5"
    property color secondaryTextColor: "#a1a1aa"
    property color borderColor: "#52525b"
    property color focusColor: "#f26a21"
    // Show the combo even when only one account matches. By default a single match
    // auto-selects and hides (no choice to make); consumers that want the chosen
    // holding always visible set this true.
    property bool showWhenSingle: false
    // Horizontal alignment of the selector's text (closed combo, dropdown rows and
    // the empty "No funds" label). Defaults to left; consumers can right-align it.
    property int textAlignment: Text.AlignLeft
    property int modelRevision: 0

    readonly property bool criteriaReady: root.accountType.length > 0
                                                  && (root.stateField.length === 0
                                                      || root.scalarText(root.stateValue).length > 0)
    readonly property var matchingAccounts: root.filteredAccounts()
    readonly property var choices: root.choiceRows()
    readonly property bool hasFunds: root.matchingAccounts.length > 0
    readonly property bool selectionValid: root.accountById(root.selectedAccountId) !== null
    readonly property bool ready: root.criteriaReady
                                  && (root.selectionValid
                                      || (root.selectionMode === ProgramAccountSelector.Output
                                          && root.createNewSelected))
    readonly property var selectedAccount: root.accountById(root.selectedAccountId)
    readonly property string selectedBalanceRaw: root.selectedAccount
                                                        ? String(root.valueFor(
                                                                     root.selectedAccount,
                                                                     "balanceRaw") || "0")
                                                        : "0"
    readonly property bool showCombo: root.selectionMode === ProgramAccountSelector.Output
                                      || (root.criteriaReady
                                          && root.matchingAccounts.length
                                             > (root.showWhenSingle ? 0 : 1))
    readonly property bool showEmptyInput: root.selectionMode === ProgramAccountSelector.Input
                                           && root.criteriaReady
                                           && root.matchingAccounts.length === 0

    signal selectionChanged(string accountId, bool createNew)

    implicitWidth: 220
    implicitHeight: root.showCombo ? 34 : root.showEmptyInput ? 20 : 0
    visible: implicitHeight > 0

    Instantiator {
        id: rows

        model: root.sourceModel
        delegate: QtObject {
            required property var model
            required property var modelData

            readonly property var accountRow: {
                if (modelData !== null && typeof modelData === "object") {
                    return modelData
                }
                if (model === null || typeof model !== "object")
                    return ({})
                return {
                    "accountId": model.accountId,
                    "address": model.address,
                    "displayAddress": model.displayAddress,
                    "accountType": model.accountType,
                    "definitionId": model.definitionId,
                    "balanceRaw": model.balanceRaw,
                    "state": model.state
                }
            }
        }
        onObjectAdded: function(index, object) {
            ++root.modelRevision
            Qt.callLater(root.reconcileSelection)
        }
        onObjectRemoved: function(index, object) {
            ++root.modelRevision
            Qt.callLater(root.reconcileSelection)
        }
    }

    onSourceModelChanged: Qt.callLater(root.reconcileSelection)
    onAccountTypeChanged: Qt.callLater(root.reconcileSelection)
    onStateFieldChanged: Qt.callLater(root.reconcileSelection)
    onStateValueChanged: Qt.callLater(root.reconcileSelection)
    onSelectionModeChanged: Qt.callLater(root.reconcileSelection)
    Component.onCompleted: Qt.callLater(root.reconcileSelection)

    Text {
        anchors.fill: parent
        visible: root.showEmptyInput
        text: root.emptyInputText
        color: root.secondaryTextColor
        font.pixelSize: 11
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: root.textAlignment
        Accessible.role: Accessible.StaticText
        Accessible.name: text
    }

    ComboBox {
        id: accountCombo

        objectName: "programAccountComboBox"
        anchors.fill: parent
        visible: root.showCombo
        enabled: root.enabled && root.criteriaReady && root.choices.length > 0
        model: root.choices
        currentIndex: root.choiceIndex()
        displayText: root.displayLabel()
        leftPadding: 10
        rightPadding: 28
        topPadding: 0
        bottomPadding: 0
        hoverEnabled: true
        activeFocusOnTab: true
        focusPolicy: Qt.StrongFocus
        Accessible.name: root.accessibleName

        contentItem: Text {
            leftPadding: accountCombo.leftPadding
            rightPadding: accountCombo.rightPadding
            text: accountCombo.displayText
            color: accountCombo.enabled ? root.textColor : root.secondaryTextColor
            font.pixelSize: 11
            verticalAlignment: Text.AlignVCenter
            horizontalAlignment: root.textAlignment
            elide: Text.ElideMiddle
        }

        indicator: Text {
            x: accountCombo.width - width - 10
            y: Math.round((accountCombo.height - height) / 2)
            text: "\u25BE"
            color: accountCombo.enabled ? root.secondaryTextColor : root.borderColor
            font.pixelSize: 10
        }

        background: Rectangle {
            radius: 7
            color: !accountCombo.enabled
                   ? root.backgroundColor
                   : accountCombo.down || accountCombo.hovered
                     ? root.hoverColor : root.backgroundColor
            border.color: accountCombo.activeFocus ? root.focusColor : root.borderColor
            border.width: 1
        }

        delegate: ItemDelegate {
            id: optionDelegate

            required property int index
            required property var modelData

            width: ListView.view ? ListView.view.width : accountCombo.width
            height: 34
            hoverEnabled: true
            highlighted: accountCombo.highlightedIndex === optionDelegate.index

            contentItem: Text {
                leftPadding: 8
                rightPadding: 8
                text: root.labelFor(optionDelegate.modelData)
                color: root.textColor
                font.pixelSize: 11
                verticalAlignment: Text.AlignVCenter
                horizontalAlignment: root.textAlignment
                elide: Text.ElideMiddle
            }

            background: Rectangle {
                radius: 5
                color: optionDelegate.highlighted || optionDelegate.hovered
                       ? root.hoverColor : "transparent"
            }
        }

        popup: Popup {
            y: accountCombo.height + 4
            width: accountCombo.width
            implicitHeight: Math.min(contentItem.implicitHeight + topPadding + bottomPadding,
                                     204)
            padding: 4
            closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

            contentItem: ListView {
                clip: true
                implicitHeight: contentHeight
                model: accountCombo.delegateModel
                currentIndex: accountCombo.highlightedIndex
                highlightMoveDuration: 0
                ScrollIndicator.vertical: ScrollIndicator { }
            }

            background: Rectangle {
                radius: 7
                color: root.backgroundColor
                border.color: root.borderColor
                border.width: 1
            }
        }

        onActivated: function(index) {
            const choice = root.choices[index]
            if (!choice)
                return
            root.setSelection(choice.createNew === true
                              ? "" : root.accountIdFor(choice),
                              choice.createNew === true)
        }
    }

    function filteredAccounts() {
        root.modelRevision
        if (!root.criteriaReady)
            return []
        const result = []
        for (let index = 0; index < rows.count; ++index) {
            const object = rows.objectAt(index)
            const row = object ? root.valueFor(object, "accountRow") : null
            if (!row)
                continue
            const type = String(root.valueFor(row, "accountType")
                                || root.valueFor(row, "typeName")
                                || root.valueFor(row, "programType") || "")
            if (type !== root.accountType)
                continue
            if (root.stateField.length > 0
                    && root.scalarText(root.valueFor(row, root.stateField))
                       !== root.scalarText(root.stateValue)) {
                continue
            }
            if (root.accountIdFor(row).length > 0)
                result.push(row)
        }
        return result
    }

    function valueFor(row, field) {
        if (!row)
            return undefined
        if (row[field] !== undefined)
            return row[field]
        if (row.state && row.state[field] !== undefined)
            return row.state[field]
        if (row.fields && row.fields[field] !== undefined)
            return row.fields[field]
        return undefined
    }

    function scalarText(value) {
        return value === undefined || value === null ? "" : String(value)
    }

    function accountIdFor(row) {
        return String(root.valueFor(row, "accountId")
                      || root.valueFor(row, "displayAddress")
                      || root.valueFor(row, "address")
                      || root.valueFor(row, "holdingId") || "")
    }

    function accountById(accountId) {
        const value = String(accountId || "")
        if (value.length === 0)
            return null
        for (let index = 0; index < root.matchingAccounts.length; ++index) {
            if (root.accountIdFor(root.matchingAccounts[index]) === value)
                return root.matchingAccounts[index]
        }
        return null
    }

    function choiceRows() {
        const result = root.matchingAccounts.slice(0)
        if (root.selectionMode === ProgramAccountSelector.Output)
            result.push({ "createNew": true })
        return result
    }

    function choiceIndex() {
        if (root.createNewSelected)
            return root.choices.length - 1
        for (let index = 0; index < root.choices.length; ++index) {
            if (root.accountIdFor(root.choices[index]) === root.selectedAccountId)
                return index
        }
        return -1
    }

    function displayLabel() {
        const index = root.choiceIndex()
        if (index >= 0)
            return root.labelFor(root.choices[index])
        if (!root.criteriaReady)
            return root.criteriaPendingText
        return root.placeholderText
    }

    function labelFor(row) {
        if (row && row.createNew === true)
            return root.createNewText
        const id = root.accountIdFor(row)
        const balanceValue = root.valueFor(row, "balanceRaw")
        const balance = balanceValue === undefined || balanceValue === null ? "" : String(balanceValue)
        return balance.length > 0
                ? qsTr("%1 · %2").arg(root.shortId(id)).arg(balance)
                : root.shortId(id)
    }

    function shortId(value) {
        const text = String(value || "")
        return text.length > 14 ? text.slice(0, 7) + "..." + text.slice(-5) : text
    }

    function setSelection(accountId, createNew) {
        const nextId = String(accountId || "")
        const nextCreate = createNew === true
        if (root.selectedAccountId === nextId && root.createNewSelected === nextCreate)
            return
        root.selectedAccountId = nextId
        root.createNewSelected = nextCreate
        root.selectionChanged(nextId, nextCreate)
    }

    function reconcileSelection() {
        if (!root.criteriaReady) {
            root.setSelection("", false)
            return
        }
        if (root.selectionValid)
            return
        if (root.selectionMode === ProgramAccountSelector.Input) {
            root.setSelection(root.matchingAccounts.length === 1
                              ? root.accountIdFor(root.matchingAccounts[0]) : "",
                              false)
            return
        }
        if (root.createNewSelected)
            return
        if (root.matchingAccounts.length === 0) {
            root.setSelection("", true)
        } else if (root.matchingAccounts.length === 1) {
            root.setSelection(root.accountIdFor(root.matchingAccounts[0]), false)
        } else {
            root.setSelection("", false)
        }
    }
}
