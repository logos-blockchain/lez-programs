pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/components/liquidity" as Liquidity

TestCase {
    id: testCase

    name: "ResponsivePopups"

    Liquidity.AmmTheme {
        id: theme
    }

    Component {
        id: viewportComponent

        Item {
            width: 320
            height: 300
        }
    }

    Component {
        id: tokenSelectorComponent

        Liquidity.TokenSelectorModal {
            theme: theme
        }
    }

    function test_tokenSelectorStaysInsideShortViewport() {
        var viewport = createTemporaryObject(viewportComponent, testCase)
        var selector = createTemporaryObject(tokenSelectorComponent, viewport, {
            "parent": viewport
        })
        verify(viewport)
        verify(selector)

        verify(selector.x >= 0)
        verify(selector.y >= 0)
        verify(selector.x + selector.width <= viewport.width)
        verify(selector.y + selector.height <= viewport.height)
    }
}
