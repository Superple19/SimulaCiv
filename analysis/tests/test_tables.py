import pandas as pd

from simciv_analysis.tables import dataframe_to_markdown, format_value


def test_number_format_and_stable_markdown_table():
    frame = pd.DataFrame({"count": [1234, None], "ratio": [0.5, 1.25], "label": ["a|b", "x\ny"]})
    assert dataframe_to_markdown(frame) == (
        "| count | ratio | label  |\n"
        "| ----- | ----- | ------ |\n"
        "| 1,234 | 0.5   | a\\|b   |\n"
        "|       | 1.25  | x<br>y |"
    )
    assert format_value(float("nan")) == ""
