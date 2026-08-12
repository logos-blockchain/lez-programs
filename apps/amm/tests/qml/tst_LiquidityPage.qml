pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/pages" as Pages

TestCase {
    id: testCase

    name: "LiquidityPage"

    Component {
        id: pageComponent

        Pages.LiquidityPage {
            visible: false
            width: 800
            height: 600
        }
    }

    function test_positionLayoutUsesDesktopRailAndCompactProgress() {
        var page = createTemporaryObject(pageComponent, testCase)
        verify(page)
        page.visible = true
        wait(0)

        var rail = findChild(page, "positionStepRail")
        var compactSteps = findChild(page, "compactPositionSteps")
        var form = findChild(page, "newPositionForm")
        verify(rail)
        verify(compactSteps)
        verify(form)

        compare(page.wideLayout, true)
        verify(rail.width > 0)
        verify(form.width > rail.width)

        page.width = 600
        wait(0)

        compare(page.wideLayout, false)
        verify(compactSteps.implicitHeight > 0)
        verify(form.width > 0)
        verify(form.width <= page.width - 32)
    }
}
