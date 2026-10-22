import matplotlib.pyplot as plt

from simciv_analysis.figures import configure, svg_bytes


def test_svg_render_is_byte_deterministic():
    configure()
    first, first_axis = plt.subplots()
    second, second_axis = plt.subplots()
    first_axis.plot([0, 1, 2], [2, 1, 3], label="fixed")
    second_axis.plot([0, 1, 2], [2, 1, 3], label="fixed")
    try:
        assert svg_bytes(first) == svg_bytes(second)
    finally:
        plt.close(first)
        plt.close(second)
