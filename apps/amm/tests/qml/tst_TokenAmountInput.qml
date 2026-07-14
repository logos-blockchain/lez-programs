pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/components/liquidity" as Liquidity

TestCase {
    id: testCase

    name: "TokenAmountInput"
    readonly property string enabledId: "22222222222222222222222222222222"
    readonly property string disabledId: "33333333333333333333333333333333"

    Component {
        id: inputComponent

        Liquidity.TokenAmountInput {
            visible: false
            width: 400
            theme: inputTheme
            tokens: [
                {
                    "definitionId": testCase.disabledId,
                    "name": "Disabled",
                    "selectable": false,
                    "code": "token_not_fungible"
                },
                {
                    "definitionId": testCase.enabledId,
                    "name": "Enabled",
                    "selectable": true
                }
            ]

            Liquidity.AmmTheme {
                id: inputTheme
            }
        }
    }

    SignalSpy {
        id: commitSpy
        signalName: "editingCommitted"
    }

    SignalSpy {
        id: selectedSpy
        signalName: "tokenSelected"
    }

    SignalSpy {
        id: enteredSpy
        signalName: "tokenEntered"
    }

    function test_inactivityCommitsLatestEditOnce() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        commitSpy.target = input
        commitSpy.clear()

        input.amountEdited("1")
        wait(150)
        compare(commitSpy.count, 0)

        input.amountEdited("12")
        wait(200)
        compare(commitSpy.count, 0)
        tryCompare(commitSpy, "count", 1, 150)
        compare(commitSpy.signalArguments[0][0], "12")
    }

    function test_editingFinishedCommitsWithoutDuplicate() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        commitSpy.target = input
        commitSpy.clear()

        input.amountEdited("7")
        input.amountEditingFinished("7")
        compare(commitSpy.count, 1)
        compare(commitSpy.signalArguments[0][0], "7")

        wait(300)
        compare(commitSpy.count, 1)
    }

    function test_compactWidthShrinksTokenAccessory() {
        var input = createTemporaryObject(inputComponent, testCase, { "width": 296 })
        verify(input)
        compare(input.accessoryWidth, 132)

        input.width = 416
        compare(input.accessoryWidth, 180)
    }

    function test_disabledTokenIsRejectedByTypedInput() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        selectedSpy.target = input
        enteredSpy.target = input
        selectedSpy.clear()
        enteredSpy.clear()

        input.acceptInput("Disabled")

        compare(selectedSpy.count, 0)
        compare(enteredSpy.count, 1)
        compare(enteredSpy.signalArguments[0][0], disabledId)
    }

    function test_enabledTokenIsSelectedByTypedInput() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        selectedSpy.target = input
        enteredSpy.target = input
        selectedSpy.clear()
        enteredSpy.clear()

        input.acceptInput("Enabled")

        compare(selectedSpy.count, 1)
        compare(selectedSpy.signalArguments[0][0], enabledId)
        compare(enteredSpy.count, 0)
    }

    function test_partialNameSelectsSoleMatchInsteadOfCustomAddress() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        selectedSpy.target = input
        enteredSpy.target = input
        selectedSpy.clear()
        enteredSpy.clear()
        input.query = "Enabl"
        tryCompare(input.rows, "length", 1)

        input.acceptInput("Enabl")

        compare(selectedSpy.count, 1)
        compare(selectedSpy.signalArguments[0][0], enabledId)
        compare(enteredSpy.count, 0)
    }

    function test_disabledTokenCanExplainWhyItIsUnavailable() {
        var input = createTemporaryObject(inputComponent, testCase, {
            "visible": true,
            "width": 320
        })
        verify(input)

        input.popup.open()
        tryCompare(input.popup, "visible", true)
        var tokenList = findChild(input, "tokenList")
        verify(tokenList)
        tryCompare(tokenList, "count", 2)
        tryVerify(function() { return tokenList.itemAtIndex(0) !== null })
        var option = tokenList.itemAtIndex(0)

        mouseMove(option, option.width / 2, option.height / 2)
        tryCompare(option, "pointerHovered", true)
        compare(option.disabledReasonVisible, true)

        selectedSpy.target = input
        enteredSpy.target = input
        selectedSpy.clear()
        enteredSpy.clear()
        mouseClick(option, option.width / 2, option.height / 2)
        compare(selectedSpy.count, 0)
        compare(enteredSpy.count, 0)
    }

    function test_tokenListSupportsKeyboardSelection() {
        var input = createTemporaryObject(inputComponent, testCase, {
            "visible": true,
            "width": 320
        })
        verify(input)
        selectedSpy.target = input
        selectedSpy.clear()

        input.popup.open()
        tryCompare(input.popup, "visible", true)
        var tokenList = findChild(input, "tokenList")
        verify(tokenList)
        tryVerify(function() { return tokenList.itemAtIndex(1) !== null })
        var option = tokenList.itemAtIndex(1)
        option.forceActiveFocus()
        tryCompare(option, "activeFocus", true)

        keyClick(Qt.Key_Space)

        compare(selectedSpy.count, 1)
        compare(selectedSpy.signalArguments[0][0], enabledId)
    }

    function test_disabledReasonAppearsOnKeyboardFocus() {
        var input = createTemporaryObject(inputComponent, testCase, {
            "visible": true,
            "width": 320
        })
        verify(input)

        input.popup.open()
        tryCompare(input.popup, "visible", true)
        var tokenList = findChild(input, "tokenList")
        verify(tokenList)
        tryVerify(function() { return tokenList.itemAtIndex(0) !== null })
        var option = tokenList.itemAtIndex(0)
        option.forceActiveFocus()

        tryCompare(option, "activeFocus", true)
        compare(option.disabledReasonVisible, true)
    }

    function test_searchModelContainsOnlyMatchingRows() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        compare(input.rows.length, 2)

        input.query = "Enabled"

        tryVerify(function() { return input.rows.length === 1 })
        compare(input.rows[0].definitionId, enabledId)
    }

    function test_typingKeepsSearchTextWhileModelFilters() {
        var input = createTemporaryObject(inputComponent, testCase, {
            "visible": true,
            "width": 320
        })
        verify(input)
        input.popup.open()
        tryCompare(input.popup, "visible", true)
        var editor = findChild(input, "tokenSearchField")
        verify(editor)

        tryCompare(editor, "activeFocus", true)
        keyClick(Qt.Key_E)
        keyClick(Qt.Key_N)
        keyClick(Qt.Key_A)
        keyClick(Qt.Key_B)
        keyClick(Qt.Key_L)
        keyClick(Qt.Key_E)
        keyClick(Qt.Key_D)

        compare(editor.text.toLowerCase(), "enabled")
        compare(input.rows.length, 1)
        compare(input.rows[0].definitionId, enabledId)
    }

    function test_clickOpensSharedTokenModal() {
        var input = createTemporaryObject(inputComponent, testCase, {
            "visible": true,
            "width": 320
        })
        verify(input)
        var button = findChild(input, "tokenSelectButton")
        verify(button)

        button.click()

        tryCompare(input.popup, "visible", true)
        verify(findChild(input, "tokenSearchField"))
        verify(findChild(input, "tokenList"))
    }

    function test_unlistedAddressCanBeEntered() {
        var input = createTemporaryObject(inputComponent, testCase)
        verify(input)
        enteredSpy.target = input
        enteredSpy.clear()
        var unlistedId = "44444444444444444444444444444444"

        input.acceptInput(unlistedId)

        compare(enteredSpy.count, 1)
        compare(enteredSpy.signalArguments[0][0], unlistedId)
    }
}
